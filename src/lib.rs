mod googleapis;

pub mod firestore {
    // TODO firestore access functions
}

pub mod bevy {
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
    pub struct BevyFirebasePlugin {
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
    struct IdToken(String);

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

    impl Plugin for BevyFirebasePlugin {
        fn build(&self, app: &mut App) {

            // TODO read keys from file
            // TODO optionally save refresh token to file

            app.add_plugin(PecsPlugin)
                .insert_resource(GoogleClientId(self.google_client_id.clone()))
                .insert_resource(GoogleClientSecret(
                    self.google_client_secret.clone(),
                ))
                .insert_resource(ApiKey(self.firebase_api_key.clone()))
                .insert_resource(ProjectId(self.firebase_project_id.clone()));

            if self.firebase_refresh_token.is_some() {
                app.insert_resource(RefreshToken(
                    self.firebase_refresh_token.clone(),
                ))
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

    fn init_login(
        mut commands: Commands,
        google_client_id: Res<GoogleClientId>,
    ) {
        // sets up redirect server
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        commands.insert_resource(RedirectPort(port));

        let authorize_url = Url::parse(&*format!("https://accounts.google.com/o/oauth2/v2/auth?scope=openid profile email&response_type=code&redirect_uri=http://127.0.0.1:{}&client_id={}",port, google_client_id.0)).unwrap();

        commands.insert_resource(AuthorizeUrl(authorize_url));

        let thread_pool = AsyncComputeTaskPool::get();

        let task = thread_pool.spawn(async move {
            let mut code: Option<String> = None;

            for stream in listener.incoming() {
                match stream {
                    Ok(mut stream) => {
                        println!("in tcp listener accept");

                        {
                            // pretty much a black box to me
                            let mut reader = BufReader::new(&stream);
                            let mut request_line = String::new();
                            reader.read_line(&mut request_line).unwrap();

                            let redirect_url = request_line.split_whitespace().nth(1).unwrap(); // idk what this do
                            let url = Url::parse(&("http://localhost".to_string() + redirect_url))
                                .unwrap();

                            let code_pair = url.query_pairs().find(|pair| {
                                let &(ref key, _) = pair;
                                key == "code"
                            });

                            if code_pair.is_some() {
                                println!("Code is some! {:?}", code_pair);
                                let (_, value) = code_pair.unwrap();

                                code = Some(value.into_owned());
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

        commands
            .spawn_empty()
            .insert(RedirectTask(task));
    }

    fn poll_redirect_task(mut commands: Commands, mut q_task: Query<(Entity, &mut RedirectTask)>) {

        if q_task.is_empty() {
            return
        }

        let (e, mut task) = q_task.single_mut();
        if task.0.is_finished() {

            commands.entity(e)
            .despawn();

            let auth_code = future::block_on(future::poll_once(&mut task.0));
            
            commands.promise(|| auth_code.unwrap())
            .then(asyn!(|auth_code,
                google_client_secret: Res<GoogleClientSecret>,
                google_client_id: Res<GoogleClientId>,
                redirect_port: Res<RedirectPort>|{
                asyn::http::post(format!("https://www.googleapis.com/oauth2/v3/token"))
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
        if id_token.is_some() {
            let id_token = id_token.unwrap();
            if id_token.is_added() {
              println!("ID TOKEN: {}", id_token.0);
            }
        }
    }
}
