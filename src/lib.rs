mod googleapis;

// re-exports
// TODO find out if this is the right way of doing things
pub mod deps {
    use crate::googleapis;

    pub use tonic::Status;

    pub use googleapis::google::firestore::v1::listen_response::ResponseType;
    pub use googleapis::google::firestore::v1::ListenResponse;
    pub use googleapis::google::firestore::v1::{value::ValueType, Value};
}

use std::{
    collections::HashMap,
    fs::{read_to_string, remove_file, write},
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
    path::PathBuf,
};

use serde::Deserialize;
use url::Url;

use crate::googleapis::google::firestore::v1::{
    firestore_client::FirestoreClient,
    listen_request::TargetChange,
    target::{DocumentsTarget, TargetType},
    CreateDocumentRequest, DeleteDocumentRequest, Document, GetDocumentRequest, ListenRequest,
    ListenResponse, Target, UpdateDocumentRequest, Value,
};

use bevy::prelude::*;

use bevy_tokio_tasks::TokioTasksRuntime;

use futures_lite::{stream, StreamExt};

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

#[derive(Resource, Clone)]
struct EmulatorUrl(String);

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
pub enum FirestoreState {
    #[default]
    Start,
    Init,
    CreateClient,
    Ready,
}

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
            .add_system(logged_in.in_schedule(OnEnter(AuthState::LoggedIn)))
            .add_system(init.in_schedule(OnEnter(FirestoreState::Init)))
            .add_system(create_client.in_schedule(OnEnter(FirestoreState::CreateClient)));
    }
}

fn logged_in(mut next_state: ResMut<NextState<FirestoreState>>) {
    next_state.set(FirestoreState::Init);
}

fn init(mut next_state: ResMut<NextState<FirestoreState>>) {
    next_state.set(FirestoreState::CreateClient);
}

fn create_client(
    runtime: ResMut<TokioTasksRuntime>,
    user_info: Res<TokenData>,
    emulator: Option<Res<EmulatorUrl>>,
    project_id: Res<ProjectId>,
) {
    let id_token = user_info.id_token.clone();
    let project_id = project_id.0.clone();

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

pub trait ListenerEventBuilder {
    fn new(msg: ListenResponse) -> Self;
}

pub fn add_listener<T>(
    runtime: &ResMut<TokioTasksRuntime>,
    client: &mut BevyFirestoreClient,
    project_id: String,
    target: String,
) where
    T: ListenerEventBuilder + std::marker::Send + std::marker::Sync + 'static,
{
    let mut client = client.0.clone();
    runtime.spawn_background_task(|mut ctx| async move {
        let db = format!("projects/{project_id}/databases/(default)");
        let req = ListenRequest {
            database: db.clone(),
            labels: HashMap::new(),
            target_change: Some(TargetChange::AddTarget(Target {
                target_id: 0x52757374, // rust in hex, for... reasons?
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
            ctx.run_on_main_thread(move |ctx| {
                ctx.world.send_event(T::new(msg.unwrap()));
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

// AUTH

// From plugin
#[derive(Resource)]
struct GoogleClientId(String);

// From plugin
#[derive(Resource)]
struct GoogleClientSecret(String);

// From plugin
#[derive(Resource)]
struct ApiKey(String);

// From plugin
#[derive(Resource)]
pub struct ProjectId(pub String);

// TODO big struct that holds all of the returned info from auth

// Retrieved
#[derive(Deserialize, Resource, Default)]
pub struct TokenData {
    #[serde(rename = "localId")]
    #[serde(alias = "user_id")]
    pub local_id: String,
    #[serde(rename = "emailVerified")]
    pub email_verified: Option<bool>,
    #[serde(rename = "email")]
    pub email: Option<String>,
    #[serde(rename = "oauthIdToken")]
    pub oauth_id_token: Option<String>,
    #[serde(rename = "oauthAccessToken")]
    pub oauth_access_token: Option<String>,
    #[serde(rename = "oauthTokenSecret")]
    pub oauth_token_secret: Option<String>,
    #[serde(rename = "rawUserInfo")]
    pub raw_user_info: Option<String>,
    #[serde(rename = "firstName")]
    pub first_name: Option<String>,
    #[serde(rename = "lastName")]
    pub last_name: Option<String>,
    #[serde(rename = "fullName")]
    pub full_name: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(rename = "photoUrl")]
    pub photo_url: Option<String>,
    #[serde(rename = "idToken")]
    #[serde(alias = "id_token")]
    pub id_token: String,
    #[serde(rename = "refreshToken")]
    #[serde(alias = "refresh_token")]
    pub refresh_token: String,
    #[serde(rename = "expiresIn")]
    #[serde(alias = "expires_in")]
    pub expires_in: String,
}

// Retrieved
#[derive(Resource)]
struct GoogleToken(String);

// Retrieved
#[derive(Resource)]
struct GoogleAuthCode(String);

// Generated
#[derive(Resource)]
struct RedirectPort(u16);

// Event
pub struct GotAuthUrl(pub Url);

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
pub enum AuthState {
    #[default]
    LoggedOut,
    LogOut,
    Refreshing,
    LogIn,
    GotAuthCode,
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

        app.insert_resource(GoogleClientId(self.google_client_id.clone()))
            .insert_resource(GoogleClientSecret(self.google_client_secret.clone()))
            .insert_resource(ApiKey(self.firebase_api_key.clone()))
            .insert_resource(ProjectId(self.firebase_project_id.clone()))
            .insert_resource(TokenData::default())
            .add_state::<AuthState>()
            .add_event::<GotAuthUrl>()
            .add_system(init_login.in_schedule(OnEnter(AuthState::LogIn)))
            .add_system(auth_code_to_firebase_token.in_schedule(OnEnter(AuthState::GotAuthCode)))
            .add_system(refresh_login.in_schedule(OnEnter(AuthState::Refreshing)))
            .add_system(save_refresh_token.in_schedule(OnEnter(AuthState::LoggedIn)))
            .add_system(login_clear_resources.in_schedule(OnEnter(AuthState::LogOut)))
            .add_system(logout_clear_resources.in_schedule(OnEnter(AuthState::LogOut)));

        if self.firebase_refresh_token.is_some() {
            app.insert_resource(TokenData {
                refresh_token: self.firebase_refresh_token.clone().unwrap(),
                ..Default::default()
            });
        }
    }
}

// designed to be called on user managed state change
// but ofc can be passed params
pub fn log_in(
    current_state: Res<State<AuthState>>,
    mut next_state: ResMut<NextState<AuthState>>,
    token_data: Res<TokenData>,
) {
    if current_state.0 != AuthState::LoggedOut {
        return;
    }

    if token_data.refresh_token.clone().is_empty() {
        next_state.set(AuthState::LogIn);
    } else {
        next_state.set(AuthState::Refreshing);
    }
}

pub fn log_out(current_state: Res<State<AuthState>>, mut next_state: ResMut<NextState<AuthState>>) {
    if current_state.0 == AuthState::LoggedOut {
        return;
    }

    next_state.set(AuthState::LogOut);
}

fn logout_clear_resources(mut commands: Commands, mut next_state: ResMut<NextState<AuthState>>) {
    commands.remove_resource::<TokenData>();

    let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
    let _ = remove_file(data_dir.join("bevy-firebase/keys/firebase-refresh.key"));

    next_state.set(AuthState::LoggedOut);

    println!("Logged out.");
}

fn login_clear_resources(mut commands: Commands) {
    commands.remove_resource::<RedirectPort>();
    commands.remove_resource::<GoogleToken>();
    commands.remove_resource::<GoogleAuthCode>();
}

fn init_login(
    mut commands: Commands,
    google_client_id: Res<GoogleClientId>,
    mut ew: EventWriter<GotAuthUrl>,
    runtime: ResMut<TokioTasksRuntime>,
) {
    // sets up redirect server
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    commands.insert_resource(RedirectPort(port));

    let authorize_url = Url::parse(&format!("https://accounts.google.com/o/oauth2/v2/auth?scope=openid profile email&response_type=code&redirect_uri=http://127.0.0.1:{}&client_id={}",port, google_client_id.0)).unwrap();

    ew.send(GotAuthUrl(authorize_url));

    runtime.spawn_background_task(|mut ctx| async move {
        // TODO fix blocking that prevents closing the app ???

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    {
                        // pretty much a black box to me
                        let mut reader = BufReader::new(&stream);
                        let mut request_line = String::new();
                        reader.read_line(&mut request_line).unwrap();

                        let redirect_url = request_line.split_whitespace().nth(1).unwrap(); // idk what this do
                        let url = Url::parse(&("http://localhost".to_string() + redirect_url));

                        let url = url.unwrap().to_owned();

                        let code_pair = url.query_pairs().find(|pair| {
                            let (key, _) = pair;
                            key == "code"
                        });

                        if let Some(code_pair) = code_pair {
                            let code = code_pair.1.into_owned();
                            ctx.run_on_main_thread(move |ctx| {
                                ctx.world.insert_resource(GoogleAuthCode(code));

                                ctx.world
                                    .insert_resource(NextState(Some(AuthState::GotAuthCode)));
                            })
                            .await;
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
                    ctx.sleep_updates(60).await;
                    continue;
                }
                Err(e) => {
                    panic!("IO_ERR: {:?}", e);
                }
            }
        }
    });
}

fn auth_code_to_firebase_token(
    auth_code: Res<GoogleAuthCode>,
    runtime: ResMut<TokioTasksRuntime>,
    port: Res<RedirectPort>,
    secret: Res<GoogleClientSecret>,
    client_id: Res<GoogleClientId>,
    api_key: Res<ApiKey>,
) {
    let auth_code = auth_code.0.clone();
    let port = format!("{}", port.0);
    let secret = secret.0.clone();
    let client_id = client_id.0.clone();
    let api_key = api_key.0.clone();

    runtime.spawn_background_task(|mut ctx| async move {
        let client = reqwest::Client::new();
        let form = reqwest::multipart::Form::new()
            .text("code", auth_code)
            .text("client_id", client_id)
            .text("client_secret", secret)
            .text("redirect_uri", format!("http://127.0.0.1:{port}"))
            .text("grant_type", "authorization_code");

        #[derive(Deserialize, Debug)]
        struct GoogleTokenResponse {
            access_token: String,
        }

        // Get Google Token
        let google_token = client
            .post("https://www.googleapis.com/oauth2/v3/token")
            .multipart(form)
            .send()
            .await
            .unwrap()
            .json::<GoogleTokenResponse>()
            .await
            .unwrap();

        let access_token = google_token.access_token;

        let mut body = HashMap::new();
        body.insert(
            "postBody",
            format!("access_token={}&providerId={}", access_token, "google.com"),
        );
        body.insert("requestUri", format!("http://127.0.0.1:{port}"));
        body.insert("returnIdpCredential", "true".into());
        body.insert("returnSecureToken", "true".into());

        // Get Firebase Token
        let firebase_token = client
            .post(format!(
                "https://identitytoolkit.googleapis.com/v1/accounts:signInWithIdp?key={}",
                api_key
            ))
            .json(&body)
            .send()
            .await
            .unwrap()
            .json::<TokenData>()
            .await
            .unwrap();

        // Use Firebase Token TODO pull into fn?
        ctx.run_on_main_thread(move |ctx| {
            ctx.world.insert_resource(firebase_token);

            // Set next state
            ctx.world
                .insert_resource(NextState(Some(AuthState::LoggedIn)));
        })
        .await;
    });
}

fn save_refresh_token(token_data: Res<TokenData>) {
    let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
    let _ = write(
        data_dir.join("bevy-firebase/keys/firebase-refresh.key"),
        token_data.refresh_token.as_str(),
    );
}

fn refresh_login(
    token_data: Res<TokenData>,
    firebase_api_key: Res<ApiKey>,
    runtime: ResMut<TokioTasksRuntime>,
) {
    let refresh_token = token_data.refresh_token.clone();
    let api_key = firebase_api_key.0.clone();

    runtime.spawn_background_task(|mut ctx| async move {
        let client = reqwest::Client::new();

        let firebase_token = client
            .post(format!(
                "https://securetoken.googleapis.com/v1/token?key={}",
                api_key
            ))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!(
                "grant_type=refresh_token&refresh_token={}",
                refresh_token
            ))
            .send()
            .await
            .unwrap()
            .json::<TokenData>()
            .await
            .unwrap();

        // Use Firebase Token TODO pull into fn?
        ctx.run_on_main_thread(move |ctx| {
            ctx.world.insert_resource(firebase_token);

            // Set next state
            ctx.world
                .insert_resource(NextState(Some(AuthState::LoggedIn)));
        })
        .await;
    });
}
