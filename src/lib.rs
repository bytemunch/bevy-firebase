mod googleapis;

pub fn echo_test(s: String) {
    print!("\nEcho: {}\n", s);
}

pub mod auth {
    use std::{
        error::Error,
        fs::read_to_string,
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        path::PathBuf,
        str,
    };

    use oauth2::{
        basic::BasicClient,
        http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, Method},
        reqwest::{async_http_client, http_client},
        AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, HttpRequest,
        PkceCodeChallenge, RedirectUrl, RevocationUrl, Scope, StandardRevocableToken,
        TokenResponse, TokenUrl,
    };
    use serde_json::Value;
    use tonic::{
        codegen::InterceptedService,
        metadata::MetadataValue,
        transport::{Certificate, Channel, ClientTlsConfig},
        Request, Status,
    };
    use url::Url;

    use crate::googleapis::google::firestore::v1::firestore_client::FirestoreClient;

    pub async fn get_firebase_token(
        google_client_id: String,
        google_client_secret: String,
        firebase_api_key: String,
        auth_url_callback: impl Fn(Url) -> (),
    ) -> Value {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
            .expect("Invalid authorization endpoint URL");
        let token_url = TokenUrl::new("https://www.googleapis.com/oauth2/v3/token".to_string())
            .expect("Invalid token endpoint URL");

        // Set up the config for the Google OAuth2 process.
        let client = BasicClient::new(
            ClientId::new(google_client_id),
            Some(ClientSecret::new(google_client_secret)),
            auth_url,
            Some(token_url),
        )
        // This example will be running its own server at localhost:{port}.
        // See below for the server implementation.
        .set_redirect_uri(
            RedirectUrl::new(format!("http://localhost:{port}")).expect("Invalid redirect URL"),
        )
        // Google supports OAuth 2.0 Token Revocation (RFC-7009)
        .set_revocation_uri(
            RevocationUrl::new("https://oauth2.googleapis.com/revoke".to_string())
                .expect("Invalid revocation endpoint URL"),
        );

        // Google supports Proof Key for Code Exchange (PKCE - https://oauth.net/2/pkce/).
        // Create a PKCE code verifier and SHA-256 encode it as a code challenge.
        let (pkce_code_challenge, pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();

        // Generate the authorization URL to which we'll redirect the user.
        let (authorize_url, _csrf_state) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("openid profile email".to_string()))
            .set_pkce_challenge(pkce_code_challenge)
            .url();

        auth_url_callback(authorize_url);

        let (google_token, firebase_token);

        // A very naive implementation of the redirect server.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();

        match listener.accept() {
            Ok((mut stream, _addr)) => {
                let code;
                // let state;
                {
                    let mut reader = BufReader::new(&stream);

                    let mut request_line = String::new();
                    reader.read_line(&mut request_line).unwrap();

                    let redirect_url = request_line.split_whitespace().nth(1).unwrap();
                    let url = Url::parse(&("http://localhost".to_string() + redirect_url)).unwrap();

                    let code_pair = url
                        .query_pairs()
                        .find(|pair| {
                            let &(ref key, _) = pair;
                            key == "code"
                        })
                        .unwrap();

                    let (_, value) = code_pair;
                    code = AuthorizationCode::new(value.into_owned());
                }

                let message = "Login Complete! You can close this window.";
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{}",
                    message.len(),
                    message
                );
                stream.write_all(response.as_bytes()).unwrap();

                // Exchange the code with a token.
                google_token = client
                    .exchange_code(code)
                    .set_pkce_verifier(pkce_code_verifier)
                    .request_async(async_http_client)
                    .await;

                // Sign in to firebase
                firebase_token = get_firebase_token_from_google_access_token(
                    google_token
                        .as_ref()
                        .unwrap()
                        .access_token()
                        .secret()
                        .into(),
                    firebase_api_key,
                    port,
                )
                .await;

                // Revoke the obtained token
                let token_response = google_token.unwrap();
                let token_to_revoke: StandardRevocableToken = match token_response.refresh_token() {
                    Some(token) => token.into(),
                    None => token_response.access_token().into(),
                };

                client
                    .revoke_token(token_to_revoke)
                    .unwrap()
                    .request_async(async_http_client)
                    .await
                    .expect("Failed to revoke token");

                println!("Token revoked. Closing server...\n");

                return firebase_token;
            }
            Err(e) => {
                println!("couldn't get client: {e:?}");
                return Value::Null;
            }
        }
    }

    pub async fn create_firestore_client(
        firebase_id_token: String,
    ) -> Result<
        FirestoreClient<
            InterceptedService<Channel, impl Fn(Request<()>) -> Result<Request<()>, Status>>,
        >,
        Box<dyn Error>,
    > {
        let bearer_token = format!("Bearer {}", firebase_id_token);
        let header_value: MetadataValue<_> = bearer_token.parse()?;

        let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
        let certs = read_to_string(data_dir.join("gcp/gtsr1.pem"))?;

        let tls_config = ClientTlsConfig::new()
            .ca_certificate(Certificate::from_pem(certs))
            .domain_name("firestore.googleapis.com");

        let channel = Channel::from_static("https://firestore.googleapis.com")
            .tls_config(tls_config)?
            .connect()
            .await?;

        let service = FirestoreClient::with_interceptor(channel, move |mut req: Request<()>| {
            req.metadata_mut()
                .insert("authorization", header_value.clone());
            Ok(req)
        });

        return Ok(service);
    }

    async fn get_firebase_token_from_google_access_token(
        google_access_token: String,
        firebase_api_key: String,
        redirect_port: u16,
    ) -> Value {
        let mut headers = HeaderMap::new();

        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str("application/json").unwrap(),
        );

        let body = format!(
            "{{\"postBody\":\"access_token={}&providerId={}\",
            \"requestUri\":\"http://127.0.0.1:{}\",
            \"returnIdpCredential\":true,
            \"returnSecureToken\":true}}",
            google_access_token, "google.com", redirect_port
        );

        let url = Url::parse(
            format!(
                "https://identitytoolkit.googleapis.com/v1/accounts:signInWithIdp?key={firebase_api_key}",
            )
            .as_str(),
        )
        .unwrap();

        let res = async_http_client(HttpRequest {
            url,
            method: Method::POST,
            headers,
            body: body.into(),
        })
        .await;

        let json = match str::from_utf8(res.unwrap().body.as_ref()) {
            Ok(v) => serde_json::from_str::<serde_json::Value>(v).unwrap(),
            Err(e) => panic!("Invalid UTF-8. {:?}", e),
        };

        json
    }

    pub async fn refresh_firebase_token(refresh_token: String, firebase_api_key: String) -> Value {
        let mut headers = HeaderMap::new();

        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str("application/x-www-form-urlencoded").unwrap(),
        );

        let body = format!("grant_type=refresh_token&refresh_token={refresh_token}");

        let url = Url::parse(
            format!("https://securetoken.googleapis.com/v1/token?key={firebase_api_key}",).as_str(),
        )
        .unwrap();

        let res = async_http_client(HttpRequest {
            url,
            method: Method::POST,
            headers,
            body: body.into(),
        })
        .await;

        let json = match str::from_utf8(res.unwrap().body.as_ref()) {
            Ok(v) => serde_json::from_str::<serde_json::Value>(v).unwrap(),
            Err(e) => panic!("Invalid UTF-8. {:?}", e),
        };

        json
    }

    pub fn refresh_firebase_token_sync(refresh_token: String, firebase_api_key: String) -> Value {
        let mut headers = HeaderMap::new();

        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str("application/x-www-form-urlencoded").unwrap(),
        );

        let body = format!("grant_type=refresh_token&refresh_token={refresh_token}");

        let url = Url::parse(
            format!("https://securetoken.googleapis.com/v1/token?key={firebase_api_key}",).as_str(),
        )
        .unwrap();

        let res = http_client(HttpRequest {
            url,
            method: Method::POST,
            headers,
            body: body.into(),
        });

        let json = match str::from_utf8(res.unwrap().body.as_ref()) {
            Ok(v) => serde_json::from_str::<serde_json::Value>(v).unwrap(),
            Err(e) => panic!("Invalid UTF-8. {:?}", e),
        };

        json
    }
}

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
    use oauth2::{
        basic::BasicClient, reqwest::http_client, AuthUrl, AuthorizationCode, Client, ClientId,
        ClientSecret, CsrfToken, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RevocationUrl,
        Scope, TokenResponse, TokenUrl,
    };
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
    struct BevyFirebaseGoogleClientId(String);

    #[derive(Resource)]
    struct BevyFirebaseGoogleClientSecret(String);

    #[derive(Resource)]
    struct BevyFirebaseApiKey(String);

    #[derive(Resource)]
    struct BevyFirebaseRefreshToken(Option<String>);

    #[derive(Resource)]
    struct BevyFirebaseIdToken(String);

    #[derive(Resource)]
    struct BevyFirebaseProjectId(String);

    #[derive(Resource)]
    struct BevyFirebaseRedirectPort(u16);

    #[derive(Resource)]
    struct BevyFirebaseAuthorizeUrl(Url);

    #[derive(Resource)]
    struct BevyFirebaseGoogleToken(String);

    #[derive(Resource)]
    struct BevyFirebaseGoogleAuthCode(AuthorizationCode);

    #[derive(Resource)]
    struct BevyFirebasePkce(PkceCodeVerifier);

    #[derive(Component)]
    struct BevyFirebaseRedirectTask(Task<String>);

    #[derive(Resource)]
    struct BevyFirebaseOauthClient(
        Client<
            oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
            oauth2::StandardTokenResponse<
                oauth2::EmptyExtraTokenFields,
                oauth2::basic::BasicTokenType,
            >,
            oauth2::basic::BasicTokenType,
            oauth2::StandardTokenIntrospectionResponse<
                oauth2::EmptyExtraTokenFields,
                oauth2::basic::BasicTokenType,
            >,
            oauth2::StandardRevocableToken,
            oauth2::StandardErrorResponse<oauth2::RevocationErrorResponseType>,
        >,
    );

    impl Plugin for BevyFirebasePlugin {
        fn build(&self, app: &mut App) {
            app.add_plugin(PecsPlugin)
                .insert_resource(BevyFirebaseGoogleClientId(self.google_client_id.clone()))
                .insert_resource(BevyFirebaseGoogleClientSecret(
                    self.google_client_secret.clone(),
                ))
                .insert_resource(BevyFirebaseApiKey(self.firebase_api_key.clone()))
                .insert_resource(BevyFirebaseProjectId(self.firebase_project_id.clone()));

            if self.firebase_refresh_token.is_some() {
                app.insert_resource(BevyFirebaseRefreshToken(
                    self.firebase_refresh_token.clone(),
                ))
                .add_startup_system(refresh_login);
            } else {
                // add startup system to prompt for login
                app.add_startup_system(init_login)
                    .add_system(poll_authorize_url)
                    .add_system(poll_pkce_and_auth)
                    .add_system(poll_redirect_task)
                    .add_system(poll_id_token);

                // TODO state for logged in/ logged out/ doing login
            }
        }
    }

    fn init_login(
        mut commands: Commands,
        google_client_id: Res<BevyFirebaseGoogleClientId>,
        google_client_secret: Res<BevyFirebaseGoogleClientSecret>,
    ) {
        // TODO this all off main thread

        // sets up redirect server
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        // listener
        //     .set_nonblocking(true)
        //     .expect("No nonblocking here kk");
        let port = listener.local_addr().unwrap().port();

        commands.insert_resource(BevyFirebaseRedirectPort(port));
        // commands.insert_resource(BevyFirebaseRedirectServer(listener));

        let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string())
            .expect("Invalid authorization endpoint URL");
        let token_url = TokenUrl::new("https://www.googleapis.com/oauth2/v3/token".to_string())
            .expect("Invalid token endpoint URL");

        // Set up the config for the Google OAuth2 process.
        let client = BasicClient::new(
            ClientId::new(google_client_id.0.clone()),
            Some(ClientSecret::new(google_client_secret.0.clone())),
            auth_url,
            Some(token_url),
        )
        .set_redirect_uri(
            RedirectUrl::new(format!("http://localhost:{port}")).expect("Invalid redirect URL"),
        )
        // Google supports OAuth 2.0 Token Revocation (RFC-7009)
        .set_revocation_uri(
            RevocationUrl::new("https://oauth2.googleapis.com/revoke".to_string())
                .expect("Invalid revocation endpoint URL"),
        );

        // Create a PKCE code verifier and SHA-256 encode it as a code challenge.
        let (pkce_code_challenge, pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();

        // Generate the authorization URL to which we'll redirect the user.
        let (authorize_url, _csrf_state) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new("openid profile email".to_string()))
            .set_pkce_challenge(pkce_code_challenge)
            .url();

        commands.insert_resource(BevyFirebaseAuthorizeUrl(authorize_url));
        commands.insert_resource(BevyFirebasePkce(pkce_code_verifier));
        commands.insert_resource(BevyFirebaseOauthClient(client));

        let thread_pool = AsyncComputeTaskPool::get();

        let task = thread_pool.spawn(async move {
            let mut code: Option<AuthorizationCode> = None;

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

                                code = Some(AuthorizationCode::new(value.into_owned()));

                                // TODO NEXT: tx/rx event streaming from inside async task. check bevy examples

                                // commands.insert_resource(BevyFirebaseGoogleAuthCode(
                                //     AuthorizationCode::new(value.into_owned()),
                                // ));
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
                        // Decide if we should exit
                        // break;
                        // Decide if we should try to accept a connection again
                        sleep(Duration::from_secs(1));
                        continue;
                    }
                    Err(e) => {
                        panic!("IO_ERR: {:?}", e);
                    }
                }
            }

            while code.is_none() {}

            code.unwrap().secret().into()
        });

        commands
            .spawn_empty()
            .insert(BevyFirebaseRedirectTask(task));

        println!("T-init-end\n");
    }

    fn poll_redirect_task(mut commands: Commands, mut q_task: Query<(Entity, &mut BevyFirebaseRedirectTask)>) {

        if q_task.is_empty() {
            return
        }

        let (e, mut task) = q_task.single_mut();
        if task.0.is_finished() {

            commands.entity(e)
            .despawn();

            let access_token = future::block_on(future::poll_once(&mut task.0));
            
            // TODO TODO TODO GET THE VALUE FROM THE FUTUREEEEEEeeeeeeee
            println!("REDIR TASK FINITO: {:?}",access_token);

            commands.insert_resource(BevyFirebaseGoogleAuthCode(AuthorizationCode::new(access_token.unwrap())));
        }
    }

    fn poll_authorize_url(url: Option<Res<BevyFirebaseAuthorizeUrl>>) {
        if let Some(url) = url {
            if url.is_added() {
                println!("Go to this URL to sign in:\n{}\n", url.0);
            }
        }
    }

    #[derive(Resource)]
    struct PkceAuthRunning;

    fn poll_pkce_and_auth(
        mut commands: Commands,
        pkce: Option<Res<BevyFirebasePkce>>,
        auth: Option<Res<BevyFirebaseGoogleAuthCode>>,
        client: Option<Res<BevyFirebaseOauthClient>>,
        id_token: Option<Res<BevyFirebaseIdToken>>,
        running: Option<Res<PkceAuthRunning>>,
    ) {
        if pkce.is_none() {
            println!("nopkce");
        }

        if auth.is_none() {
            // println!("noauth");
        }

        if client.is_none() {
            println!("noclient");
        }

        if running.is_some()
            || pkce.is_none()
            || auth.is_none()
            || client.is_none()
            || id_token.is_some()
        {
            return;
        } else {
            println!("pcke auth polled");
            commands.insert_resource(PkceAuthRunning);
        }

        let google_token = client
            .unwrap()
            .0
            .exchange_code(auth.unwrap().0.clone())
            .set_pkce_verifier(PkceCodeVerifier::new(pkce.unwrap().0.secret().into()))
            .request(http_client);

        commands.insert_resource(BevyFirebaseGoogleToken(
            google_token.unwrap().access_token().secret().into(),
        ));

        commands.promise(|| ()).then(asyn!(
            |_,
            firebase_api_key: Res<BevyFirebaseApiKey>,
            port: Res<BevyFirebaseRedirectPort>,
            google_access_token: Res<BevyFirebaseGoogleToken>| {
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
                    google_access_token.0, "google.com", port.0
                ))
                .send()
            }
        ))
        .then(asyn!(_, result, mut commands:Commands => {
            let json = serde_json::from_str::<serde_json::Value>(result.unwrap().text().unwrap()).unwrap();

            let id_token = json.get("idToken").unwrap().as_str().unwrap();

            commands.insert_resource(BevyFirebaseIdToken(id_token.into()));

            // TODO cleanup other resources
        }));
    }

    fn refresh_login(
        mut commands: Commands,
        refresh_token: Res<BevyFirebaseRefreshToken>,
        firebase_api_key: Res<BevyFirebaseApiKey>,
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

                commands.insert_resource(BevyFirebaseIdToken(id_token.into()));
            }))
        );

        // TODO if error, clear all token resources (logout)
    }

    fn poll_id_token(id_token: Option<Res<BevyFirebaseIdToken>>) {
        if id_token.is_some() {
            println!("ID TOKEN: {}", id_token.unwrap().0);
        }
    }
}
