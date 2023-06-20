mod googleapis;

pub mod firestore {

    use std::{fs::read_to_string, path::PathBuf};

    use crate::{
        auth::IdToken, googleapis::google::firestore::v1::firestore_client::FirestoreClient,
    };
    use bevy::{
        prelude::*,
        tasks::{AsyncComputeTaskPool, Task},
    };
    use futures_lite::future;
    use tonic::{
        codegen::InterceptedService,
        metadata::{Ascii, MetadataValue},
        service::Interceptor,
        transport::{Certificate, Channel, ClientTlsConfig},
    };

    #[derive(Resource)]
    struct BevyFirestoreClient(FirestoreClient<InterceptedService<Channel, AuthInterceptor>>);

    #[derive(Component)]
    struct BevyFirestoreChannelTask(Task<Channel>);

    struct AuthInterceptor {
        header_value: MetadataValue<Ascii>,
    }

    impl Interceptor for AuthInterceptor {
        fn call(
            &mut self,
            mut request: tonic::Request<()>,
        ) -> Result<tonic::Request<()>, tonic::Status> {
            request
                .metadata_mut()
                .insert("authorization", self.header_value.clone());
            Ok(request)
        }
    }

    pub struct FirestorePlugin;

    impl Plugin for FirestorePlugin {
        fn build(&self, app: &mut App) {
            // TODO create gRPC tonic client
            // TODO add client to app as resource
            // TODO refresh client token when app token is refreshed
            app.add_system(poll_id_token)
                .add_system(poll_channel_task)
                .add_system(poll_client_added);
        }
    }

    fn poll_id_token(
        mut commands: Commands,
        id_token: Option<Res<IdToken>>,
        firestore_client: Option<Res<BevyFirestoreClient>>,
        q_firestore_channel_task: Query<&BevyFirestoreChannelTask>,
    ) {
        // check for ID token && NOT FirestoreClient && && NOT already spawned a channel task
        if id_token.is_none() || firestore_client.is_some() || !q_firestore_channel_task.is_empty()
        {
            return;
        }

        println!("\nSpawned channel task!\n");

        // If we got those, create task entity that creates the channel
        // that is polled by poll_channel_task

        let thread_pool = AsyncComputeTaskPool::get();

        let task = thread_pool.spawn(async move {
            let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
            let certs = read_to_string(data_dir.join("gcp/gtsr1.pem")).unwrap();

            let tls_config = ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(certs))
                .domain_name("firestore.googleapis.com");

            let channel = Channel::from_static("https://firestore.googleapis.com")
                .tls_config(tls_config)
                .unwrap()
                .connect()
                .await
                .unwrap();

            println!("\nGot Channel! {:?}", channel);

            channel
        });

        commands
            .spawn_empty()
            .insert(BevyFirestoreChannelTask(task));
    }

    fn poll_channel_task(
        mut commands: Commands,
        firestore_client: Option<Res<BevyFirestoreClient>>,
        id_token: Option<Res<IdToken>>,
        mut q_task: Query<(Entity, &mut BevyFirestoreChannelTask)>,
    ) {
        if q_task.is_empty() || firestore_client.is_some() {
            return;
        }

        if let Some(token) = id_token {
            println!("\nCreating client!\n");

            let (e, mut task) = q_task.single_mut();

            if task.0.is_finished() {
                commands.entity(e).despawn();

                let channel = future::block_on(future::poll_once(&mut task.0));

                let bearer_token = format!("Bearer {}", token.0);
                let header_value: MetadataValue<_> = bearer_token.parse().unwrap();

                let service = FirestoreClient::with_interceptor(
                    channel.unwrap(),
                    AuthInterceptor { header_value },
                );

                commands.insert_resource(BevyFirestoreClient(service));
            }
        }
    }

    fn poll_client_added(client: Option<Res<BevyFirestoreClient>>) {
        if let Some(client) = client {
            if client.is_added() {
                println!("\nCLIENT ADDED!: {:?}\n", client.0);
            }
        }
    }
}

pub mod auth {
    use std::{
        io::{self, BufRead, BufReader, Write},
        net::TcpListener,
        thread::sleep,
        time::Duration,
    };

    use bevy::{
        prelude::*,
        tasks::{AsyncComputeTaskPool, Task},
    };

    use futures_lite::future;
    use pecs::prelude::{asyn, PecsPlugin, Promise, PromiseCommandsExtension, PromiseLikeBase};
    use url::Url;
    pub struct AuthPlugin {
        pub firebase_api_key: String,
        pub google_client_id: String,
        pub google_client_secret: String,
        pub firebase_refresh_token: Option<String>,
        pub firebase_project_id: String,
    }

    #[derive(Resource)]
    struct GoogleClientId(String);

    #[derive(Resource)]
    struct GoogleClientSecret(String);

    #[derive(Resource)]
    struct ApiKey(String);

    #[derive(Resource)]
    struct RefreshToken(Option<String>);

    #[derive(Resource)]
    pub struct IdToken(pub String);

    #[derive(Resource)]
    struct ProjectId(String);

    #[derive(Resource)]
    struct RedirectPort(u16);

    #[derive(Resource)]
    pub struct AuthorizeUrl(Url);

    #[derive(Resource)]
    struct GoogleToken(String);

    #[derive(Resource)]
    struct GoogleAuthCode(String);

    #[derive(Component)]
    struct RedirectTask(Task<String>);

    impl Plugin for AuthPlugin {
        fn build(&self, app: &mut App) {
            // allow user to read keys from file
            // TODO optionally save refresh token to file

            app.add_plugin(PecsPlugin)
                .insert_resource(GoogleClientId(self.google_client_id.clone()))
                .insert_resource(GoogleClientSecret(self.google_client_secret.clone()))
                .insert_resource(ApiKey(self.firebase_api_key.clone()))
                .insert_resource(ProjectId(self.firebase_project_id.clone()));

            if self.firebase_refresh_token.is_some() {
                app.insert_resource(RefreshToken(self.firebase_refresh_token.clone()))
                    .add_startup_system(refresh_login);
            } else {
                // add startup system to prompt for login
                app.add_startup_system(init_login)
                    .add_system(poll_authorize_url)
                    .add_system(poll_redirect_task)
                    .add_system(poll_id_token);

                // TODO state for logged in/ logged out/ doing login
            }
        }
    }

    fn init_login(mut commands: Commands, google_client_id: Res<GoogleClientId>) {
        // sets up redirect server
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        commands.insert_resource(RedirectPort(port));

        let authorize_url = Url::parse(&format!("https://accounts.google.com/o/oauth2/v2/auth?scope=openid profile email&response_type=code&redirect_uri=http://127.0.0.1:{}&client_id={}",port, google_client_id.0)).unwrap();

        commands.insert_resource(AuthorizeUrl(authorize_url));

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
                            let url = Url::parse(&("http://localhost".to_string() + redirect_url))
                                .unwrap();

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
            .then(asyn!(_, result, mut commands:Commands => {
                let json = serde_json::from_str::<serde_json::Value>(result.unwrap().text().unwrap()).unwrap();

                let id_token = json.get("idToken").unwrap().as_str().unwrap();

                commands.insert_resource(IdToken(id_token.into()));
                // TODO cleanup other resources
            }));
        }
    }

    fn poll_authorize_url(url: Option<Res<AuthorizeUrl>>) {
        if let Some(url) = url {
            if url.is_added() {
                println!("Go to this URL to sign in:\n{}\n", url.0);
                // TODO user facing button with link in app
                // TODO dev facing API to expose auth URL
            }
        }
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
            .then(asyn!(_state, result, mut commands:Commands=>{
                let json = serde_json::from_str::<serde_json::Value>(result.unwrap().text().unwrap()).unwrap();

                let id_token = json.get("id_token").unwrap().as_str().unwrap();

                commands.insert_resource(IdToken(id_token.into()));
            }))
        );

        // TODO if error, clear all token resources (logout)
    }

    fn poll_id_token(id_token: Option<Res<IdToken>>) {
        if let Some(token) = id_token {
            if token.is_added() {
                println!("ID TOKEN: {}", token.0);
            }
        }
    }
}
