mod googleapis;

// re-exports
// TODO find out if this is the right way of doing things
pub mod deps {
    use crate::googleapis;

    pub use tonic::Status;

    pub use googleapis::google::firestore::v1::{value::ValueType, Value};
}

use std::{
    collections::HashMap,
    fs::{read_to_string, remove_file, write},
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
    path::PathBuf,
    thread::sleep,
    time::Duration,
};

use url::Url;

use pecs::prelude::{asyn, PecsPlugin, Promise, PromiseCommandsExtension, PromiseLikeBase};

use crate::googleapis::google::firestore::v1::{
    firestore_client::FirestoreClient,
    listen_request::TargetChange,
    target::{DocumentsTarget, TargetType},
    CreateDocumentRequest, DeleteDocumentRequest, Document, GetDocumentRequest, ListenRequest,
    ListenResponse, Target, UpdateDocumentRequest, Value,
};

use bevy::prelude::*;

use bevy::tasks::{AsyncComputeTaskPool, Task};

use bevy_tokio_tasks::TokioTasksRuntime;

use futures_lite::{future, stream, StreamExt};

use tonic::{
    codegen::InterceptedService,
    metadata::{Ascii, MetadataValue},
    service::Interceptor,
    transport::{Certificate, Channel, ClientTlsConfig},
    Request, Response, Status,
};

// FIRESTORE

#[derive(Resource, Clone)]
pub struct BevyFirestoreClient(FirestoreClient<InterceptedService<Channel, FirebaseInterceptor>>);

#[derive(Resource)]
struct BevyFirebaseCreateClientRunning(bool);

#[derive(Resource, Clone)]
struct EmulatorUrl(String);

#[derive(Clone)]
struct FirebaseInterceptor {
    bearer_token: MetadataValue<Ascii>,
    db: MetadataValue<Ascii>,
}

impl Interceptor for FirebaseInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        request
            .metadata_mut()
            .insert("authorization", self.bearer_token.clone());

        request
            .metadata_mut()
            .insert("google-cloud-resource-prefix", self.db.clone());
        Ok(request)
    }
}

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
pub enum FirestoreState {
    #[default]
    Start,
    Init,
    CreateClient,
    Ready,
}

#[derive(Default)]
pub struct FirestorePlugin {
    pub emulator_url: Option<String>,
}

impl Plugin for FirestorePlugin {
    fn build(&self, app: &mut App) {
        // TODO refresh client token when app token is refreshed
        if self.emulator_url.is_some() {
            app.insert_resource(EmulatorUrl(self.emulator_url.clone().unwrap()));
        }

        app.add_state::<FirestoreState>()
            .add_event::<ListenerEvent>()
            .add_system(logged_in.in_schedule(OnEnter(AuthState::LoggedIn)))
            .add_system(init.in_schedule(OnEnter(FirestoreState::Init)))
            .add_system(create_client.in_schedule(OnEnter(FirestoreState::CreateClient)))
            .add_system(client_added.in_schedule(OnEnter(FirestoreState::Ready)));
    }
}

fn logged_in(mut next_state: ResMut<NextState<FirestoreState>>) {
    next_state.set(FirestoreState::Init);
}

fn init(mut commands: Commands, mut next_state: ResMut<NextState<FirestoreState>>) {
    commands.insert_resource(BevyFirebaseCreateClientRunning(false));

    next_state.set(FirestoreState::CreateClient);
}

fn create_client(
    mut commands: Commands,
    runtime: ResMut<TokioTasksRuntime>,
    id_token: Option<Res<IdToken>>,
    emulator: Option<Res<EmulatorUrl>>,
    project_id: Res<ProjectId>,
) {
    let id_token = id_token.unwrap().0.clone();
    let project_id = project_id.0.clone();

    commands.insert_resource(BevyFirebaseCreateClientRunning(true));

    let emulator_url = match emulator {
        Some(e) => Some(e.0.clone()),
        None => None,
    };

    // CREATE BG TASK TO INSERT CLIENT AS RESOURCE
    runtime.spawn_background_task(|mut ctx| async move {
        let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
        let certs = read_to_string(data_dir.join("bevy-firebase/gcp/gtsr1.pem")).unwrap();

        let channel = if emulator_url.is_none() {
            let tls_config = ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(certs))
                .domain_name("firestore.googleapis.com");

            Channel::from_static("https://firestore.googleapis.com")
                .tls_config(tls_config)
                .unwrap()
                .connect()
                .await
                .unwrap()
        } else {
            Channel::from_shared(emulator_url.unwrap())
                .unwrap()
                .connect()
                .await
                .unwrap()
        };

        let service = FirestoreClient::with_interceptor(
            channel,
            FirebaseInterceptor {
                bearer_token: format!("Bearer {}", id_token).parse().unwrap(),
                db: format!("projects/{}/databases/(default)", project_id.clone())
                    .parse()
                    .unwrap(),
            },
        );

        ctx.run_on_main_thread(move |ctx| {
            ctx.world.insert_resource(BevyFirestoreClient(service));

            ctx.world
                .insert_resource(NextState(Some(FirestoreState::Ready)));
        })
        .await;
    });
}

fn client_added(client: Res<BevyFirestoreClient>) {
    println!("Client added! {:?}\n", client.0)
}

#[derive(Debug)]
pub struct ListenerEventInner {
    pub res: ListenResponse,
    pub id: String, // HACK: used to differentiate between listeners, i don't understand how to take a custom event
}
pub struct ListenerEvent(pub ListenerEventInner);

pub fn add_listener(
    runtime: &ResMut<TokioTasksRuntime>,
    client: &mut BevyFirestoreClient,
    project_id: String,
    target: String,
    listener_id: String,
) {
    let mut client = client.0.clone();
    runtime.spawn_background_task(|mut ctx| async move {
        let db = format!("projects/{project_id}/databases/(default)");
        let req = ListenRequest {
            database: db.clone(),
            labels: HashMap::new(),
            target_change: Some(TargetChange::AddTarget(Target {
                target_id: 0x52757374,
                once: false,
                resume_type: None,
                target_type: Some(TargetType::Documents(DocumentsTarget {
                    documents: vec![db + "/documents/" + &*target],
                })),
                ..Default::default()
            })),
        };

        let req = Request::new(stream::iter(vec![req]).chain(stream::pending()));

        // TODO handle errors
        let res = client.listen(req).await.unwrap();

        let mut res = res.into_inner();

        while let Some(msg) = res.next().await {
            let listener_id = listener_id.clone();

            ctx.run_on_main_thread(move |ctx| {
                // TODO allow user defined events
                ctx.world.send_event(ListenerEvent(ListenerEventInner {
                    res: msg.unwrap(),
                    id: listener_id,
                }));
                // TODO test if awaiting here drops events
            })
            .await;
        }
    });
}

pub async fn create_document(
    client: &mut BevyFirestoreClient,
    project_id: &String,
    document_id: &String,
    collection_id: &String,
    document_data: HashMap<String, Value>,
) -> Result<Response<Document>, Status> {
    client
        .0
        .create_document(CreateDocumentRequest {
            parent: format!("projects/{project_id}/databases/(default)/documents"),
            collection_id: collection_id.into(),
            document_id: document_id.into(),
            document: Some(Document {
                fields: document_data,
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
}

pub async fn update_document(
    client: &mut BevyFirestoreClient,
    project_id: &String,
    document_path: &String,
    document_data: HashMap<String, Value>,
) -> Result<Response<Document>, Status> {
    client
        .0
        .update_document(UpdateDocumentRequest {
            document: Some(Document {
                name: format!(
                    "projects/{project_id}/databases/(default)/documents/{document_path}"
                ),
                fields: document_data,
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
}

pub async fn read_document(
    client: &mut BevyFirestoreClient,
    project_id: &String,
    document_path: &String,
) -> Result<Response<Document>, Status> {
    client
        .0
        .get_document(GetDocumentRequest {
            name: format!("projects/{project_id}/databases/(default)/documents/{document_path}"),
            ..Default::default()
        })
        .await
}

pub async fn delete_document(
    client: &mut BevyFirestoreClient,
    project_id: &String,
    document_path: &String,
) -> Result<Response<()>, Status> {
    client
        .0
        .delete_document(DeleteDocumentRequest {
            name: format!("projects/{project_id}/databases/(default)/documents/{document_path}"),
            ..Default::default()
        })
        .await
}

pub fn create_document_sync(
    mut client: BevyFirestoreClient,
    runtime: &ResMut<TokioTasksRuntime>,
    project_id: String,
    document_id: String,
    collection_id: String,
    document_data: HashMap<String, Value>,
) {
    runtime.spawn_background_task(|_ctx| async move {
        let res = client
            .0
            .create_document(CreateDocumentRequest {
                parent: format!("projects/{project_id}/databases/(default)/documents"),
                collection_id,
                document_id,
                document: Some(Document {
                    fields: document_data,
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await;

        // TODO sync fns return results to bevy somehow
        // run_on_main_thread(|ctx| {ctx.world.insertResource::<BevyFirebaseResult(res)>()?})
        println!("\n\ndata created: {:?}\n", res);
    });
}

pub fn delete_document_sync(
    mut client: BevyFirestoreClient,
    runtime: &ResMut<TokioTasksRuntime>,
    project_id: String,
    document_path: String,
) {
    runtime.spawn_background_task(|_ctx| async move {
        let res = client
            .0
            .delete_document(DeleteDocumentRequest {
                name: format!(
                    "projects/{project_id}/databases/(default)/documents/{document_path}"
                ),
                ..Default::default()
            })
            .await;

        println!("data deleted: {:?}\n", res);
    });
}

pub fn update_document_sync(
    mut client: BevyFirestoreClient,
    runtime: &ResMut<TokioTasksRuntime>,
    project_id: String,
    document_path: String,
    document_data: HashMap<String, Value>,
) {
    runtime.spawn_background_task(|_ctx| async move {
        let res = client
            .0
            .update_document(UpdateDocumentRequest {
                document: Some(Document {
                    name: format!(
                        "projects/{project_id}/databases/(default)/documents/{document_path}"
                    ),
                    fields: document_data,
                    ..Default::default()
                }),
                ..Default::default()
            })
            .await;

        println!("data updated: {:?}\n", res);
    });
}

pub fn read_document_sync(
    mut client: BevyFirestoreClient,
    runtime: &ResMut<TokioTasksRuntime>,
    project_id: String,
    document_path: String,
) {
    runtime.spawn_background_task(|_ctx| async move {
        let res = client
            .0
            .get_document(GetDocumentRequest {
                name: format!(
                    "projects/{project_id}/databases/(default)/documents/{document_path}"
                ),
                ..Default::default()
            })
            .await;

        println!("data read: {:?}\n", res);
    });
}

// AUTH

// TODO super-struct this stuff, make pub only needed fields

#[derive(Resource)]
struct GoogleClientId(String);

#[derive(Resource)]
struct GoogleClientSecret(String);

#[derive(Resource)]
struct ApiKey(String);

#[derive(Resource)]
pub struct RefreshToken(Option<String>);

#[derive(Resource)]
pub struct IdToken(pub String);

#[derive(Resource)]
pub struct UserId(pub String);

#[derive(Resource)]
pub struct ProjectId(pub String);

#[derive(Resource)]
struct RedirectPort(u16);

#[derive(Resource)]
struct GoogleToken(String);

#[derive(Resource)]
struct GoogleAuthCode(String);

#[derive(Component)]
struct RedirectTask(Task<String>);

pub struct GotAuthUrl(pub Url);

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
pub enum AuthState {
    #[default]
    LoggedOut,
    LogOut,
    Refreshing,
    LogIn,
    WaitForLogin,
    LoggedIn,
}

pub struct AuthPlugin {
    pub google_client_id: String,
    pub google_client_secret: String,
    pub firebase_api_key: String,
    pub firebase_project_id: String,
    pub firebase_refresh_token: Option<String>,
}

impl Default for AuthPlugin {
    fn default() -> Self {
        let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
        let firebase_api_key =
            read_to_string(data_dir.join("bevy-firebase/keys/firebase-api.key")).unwrap();
        let google_client_id =
            read_to_string(data_dir.join("bevy-firebase/keys/google-client-id.key")).unwrap();
        let google_client_secret =
            read_to_string(data_dir.join("bevy-firebase/keys/google-client-secret.key")).unwrap();
        let firebase_refresh_token =
            read_to_string(data_dir.join("bevy-firebase/keys/firebase-refresh.key"));

        let firebase_refresh_token = match firebase_refresh_token {
            Ok(key) => Some(key),
            Err(_) => None,
        };

        AuthPlugin {
            firebase_api_key,
            google_client_id,
            google_client_secret,
            firebase_refresh_token,
            firebase_project_id: "".into(),
        }
    }
}

impl Plugin for AuthPlugin {
    fn build(&self, app: &mut App) {
        // TODO optionally save refresh token to file

        app.add_plugin(PecsPlugin)
            .insert_resource(GoogleClientId(self.google_client_id.clone()))
            .insert_resource(GoogleClientSecret(self.google_client_secret.clone()))
            .insert_resource(ApiKey(self.firebase_api_key.clone()))
            .insert_resource(ProjectId(self.firebase_project_id.clone()))
            .insert_resource(RefreshToken(self.firebase_refresh_token.clone()))
            .add_state::<AuthState>()
            .add_event::<GotAuthUrl>()
            .add_system(init_login.in_schedule(OnEnter(AuthState::LogIn)))
            .add_system(poll_redirect_task.in_set(OnUpdate(AuthState::WaitForLogin)))
            .add_system(refresh_login.in_schedule(OnEnter(AuthState::Refreshing)))
            .add_system(save_refresh_token.in_schedule(OnEnter(AuthState::LoggedIn)))
            .add_system(cleanup.in_schedule(OnEnter(AuthState::LogOut)));
    }
}

// designed to be called on user managed state change
// but ofc can be passed params
pub fn log_in(
    current_state: Res<State<AuthState>>,
    mut next_state: ResMut<NextState<AuthState>>,
    refresh_token: Res<RefreshToken>,
) {
    if current_state.0 != AuthState::LoggedOut {
        return;
    }

    if let Some(_token) = refresh_token.0.clone() {
        next_state.set(AuthState::Refreshing);
    } else {
        next_state.set(AuthState::LogIn);
    }
}

pub fn log_out(current_state: Res<State<AuthState>>, mut next_state: ResMut<NextState<AuthState>>) {
    if current_state.0 == AuthState::LoggedOut {
        return;
    }

    next_state.set(AuthState::LogOut);
}

fn cleanup(mut commands: Commands, mut next_state: ResMut<NextState<AuthState>>) {
    commands.remove_resource::<IdToken>();
    commands.remove_resource::<UserId>();

    commands.insert_resource(RefreshToken(None));

    let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
    let _ = remove_file(data_dir.join("bevy-firebase/keys/firebase-refresh.key"));

    next_state.set(AuthState::LoggedOut);

    println!("Logged out.");
}

fn init_login(
    mut commands: Commands,
    google_client_id: Res<GoogleClientId>,
    mut ew: EventWriter<GotAuthUrl>,
    mut next_state: ResMut<NextState<AuthState>>,
) {
    // sets up redirect server
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    commands.insert_resource(RedirectPort(port));

    let authorize_url = Url::parse(&format!("https://accounts.google.com/o/oauth2/v2/auth?scope=openid profile email&response_type=code&redirect_uri=http://127.0.0.1:{}&client_id={}",port, google_client_id.0)).unwrap();

    ew.send(GotAuthUrl(authorize_url));

    // TODO use bevy-tokio-tasks here

    let thread_pool = AsyncComputeTaskPool::get();

    let task = thread_pool.spawn(async move {
        let mut code: Option<String> = None;

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    {
                        // pretty much a black box to me
                        let mut reader = BufReader::new(&stream);
                        let mut request_line = String::new();
                        reader.read_line(&mut request_line).unwrap();

                        let redirect_url = request_line.split_whitespace().nth(1).unwrap(); // idk what this do
                        let url =
                            Url::parse(&("http://localhost".to_string() + redirect_url)).unwrap();

                        let code_pair = url.query_pairs().find(|pair| {
                            let (key, _) = pair;
                            key == "code"
                        });

                        if let Some(code_pair) = code_pair {
                            code = Some(code_pair.1.into_owned());
                        }
                    }

                    // message in browser
                    // TODO allow user styling etc.
                    let message = "Login Complete! You can close this window.";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                        message.len(),
                        message
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    break;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    sleep(Duration::from_secs(1));
                    continue;
                }
                Err(e) => {
                    panic!("IO_ERR: {:?}", e);
                }
            }
        }

        while code.is_none() {}

        code.unwrap()
    });

    commands.spawn_empty().insert(RedirectTask(task));

    next_state.set(AuthState::WaitForLogin);
}

fn poll_redirect_task(mut commands: Commands, mut q_task: Query<(Entity, &mut RedirectTask)>) {
    if q_task.is_empty() {
        return;
    }

    let (e, mut task) = q_task.single_mut();
    if task.0.is_finished() {
        commands.entity(e).despawn();

        let auth_code = future::block_on(future::poll_once(&mut task.0));

        commands.promise(|| auth_code.unwrap())
            .then(asyn!(|auth_code,
                google_client_secret: Res<GoogleClientSecret>,
                google_client_id: Res<GoogleClientId>,
                redirect_port: Res<RedirectPort>|{
                asyn::http::post("https://www.googleapis.com/oauth2/v3/token")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(format!("code={}&client_id={}&client_secret={}&redirect_uri=http://127.0.0.1:{}&grant_type=authorization_code",auth_code.value,google_client_id.0, google_client_secret.0, redirect_port.0))
                .send()
            }))
            .then(asyn!(
                |_, result,
                firebase_api_key: Res<ApiKey>,
                port: Res<RedirectPort>| {

                    let json = serde_json::from_str::<serde_json::Value>(result.unwrap().text().unwrap()).unwrap();

                    let access_token = json.get("access_token").unwrap().as_str().unwrap();

                    asyn::http::post(format!(
                        "https://identitytoolkit.googleapis.com/v1/accounts:signInWithIdp?key={}",
                        firebase_api_key.0
                    ))
                    .header("content-type", "application/json")
                    .body(format!(
                        "{{\"postBody\":\"access_token={}&providerId={}\",
                        \"requestUri\":\"http://127.0.0.1:{}\",
                        \"returnIdpCredential\":true,
                        \"returnSecureToken\":true}}",
                        access_token, "google.com", port.0
                    ))
                    .send()
                }
            ))
            .then(asyn!(
                _,
                result,
                mut commands:Commands,
                mut next_state: ResMut<NextState<AuthState>>,
                 => {
                // TODO dry
                let json = serde_json::from_str::<serde_json::Value>(result.unwrap().text().unwrap()).unwrap();

                let id_token = json.get("idToken").unwrap().as_str().unwrap();
                let uid = json.get("localId").unwrap().as_str().unwrap();
                let refresh_token = json.get("refreshToken").unwrap().as_str().unwrap();

                commands.insert_resource(IdToken(id_token.into()));
                commands.insert_resource(UserId(uid.into()));
                commands.insert_resource(RefreshToken(Some(refresh_token.into())));

                commands.remove_resource::<RedirectPort>();
                commands.remove_resource::<GoogleToken>();
                commands.remove_resource::<GoogleAuthCode>();
                
                next_state.set(AuthState::LoggedIn);
            }));
    }
}

fn save_refresh_token(refresh_token: Res<RefreshToken>) {
    let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
    let _ = write(
        data_dir.join("bevy-firebase/keys/firebase-refresh.key"),
        refresh_token.0.as_ref().unwrap().as_str(),
    );
}

fn refresh_login(
    mut commands: Commands,
    refresh_token: Res<RefreshToken>,
    firebase_api_key: Res<ApiKey>,
) {
    let tokens = (refresh_token.0.clone().unwrap(), firebase_api_key.0.clone());

    commands.add(
        Promise::new(tokens, asyn!(state=>{
            asyn::http::post(format!("https://securetoken.googleapis.com/v1/token?key={}",state.1))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("grant_type=refresh_token&refresh_token={}",state.0))
            .send()
        }))
        .then(asyn!(
            _state,
            result,
            mut commands:Commands,
            mut next_state: ResMut<NextState<AuthState>>,
                => {
            // TODO dry
            // TODO dry     lol
            let json = serde_json::from_str::<serde_json::Value>(result.unwrap().text().unwrap()).unwrap();

            let id_token = json.get("id_token").unwrap().as_str().unwrap();
            let uid = json.get("user_id").unwrap().as_str().unwrap();
            let refresh_token = json.get("refresh_token").unwrap().as_str().unwrap();

            commands.insert_resource(IdToken(id_token.into()));
            commands.insert_resource(UserId(uid.into()));
            commands.insert_resource(RefreshToken(Some(refresh_token.into())));

            next_state.set(AuthState::LoggedIn);
        }))
    );
}
