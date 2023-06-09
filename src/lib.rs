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
    use std::{net::TcpListener, io::{BufReader, Write, BufRead}, str::FromStr};

    use bevy::prelude::{App, Commands, Plugin, Res, Resource};
    use oauth2::{AuthUrl, TokenUrl, basic::BasicClient, ClientId, ClientSecret, RedirectUrl, RevocationUrl, CsrfToken, Scope, PkceCodeChallenge, AuthorizationCode, reqwest::http_client, TokenResponse, PkceCodeVerifier};
    use pecs::prelude::{asyn, PecsPlugin, Promise, PromiseLikeBase};
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
    struct BevyFirebaseIdToken(String);

    #[derive(Resource)]
    struct BevyFirebaseRefreshToken(Option<String>);

    #[derive(Resource)]
    struct BevyFirebaseProjectId(String);

    #[derive(Resource)]
    struct BevyFirebaseRedirectServer(TcpListener);

    #[derive(Resource)]
    struct BevyFirebaseRedirectPort(u16);

    #[derive(Resource)]
    struct BevyFirebaseAuthorizeUrl(Url);

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
                app
                .add_startup_system(init_login);
            }
        }
    }

    fn init_login(
        mut commands: Commands,
        google_client_id: Res<BevyFirebaseGoogleClientId>,
        google_client_secret: Res<BevyFirebaseGoogleClientSecret>,
    ) {
        // sets up redirect server
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // puts redirect server in resource
        commands.insert_resource(BevyFirebaseRedirectServer(listener));
        commands.insert_resource(BevyFirebaseRedirectPort(port));

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

        // Redirect Server Bits
        let state = (AuthorizationCode::new("".into()),client,String::new(),PkceCodeVerifier::new("".into()));

        commands.add(
            Promise::new(state, asyn!(state, listener:Res<BevyFirebaseRedirectServer>, mut commands:Commands=>{
                // Google supports Proof Key for Code Exchange (PKCE - https://oauth.net/2/pkce/).
                // Create a PKCE code verifier and SHA-256 encode it as a code challenge.
                let (pkce_code_challenge, pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();

                state.3 = pkce_code_verifier;

                // Generate the authorization URL to which we'll redirect the user.
                let (authorize_url, _csrf_state) = state.1
                    .authorize_url(CsrfToken::new_random)
                    .add_scope(Scope::new("openid profile email".to_string()))
                    .set_pkce_challenge(pkce_code_challenge)
                    .url();

                println!("GOTOURL: {}",authorize_url);
                
                match listener.0.accept() {
                    Ok((mut stream, _addr)) => {
                        {// pretty much a black box to me
                            let mut reader = BufReader::new(&stream);
                            let mut request_line = String::new();
                            reader.read_line(&mut request_line).unwrap();

                            let redirect_url = request_line.split_whitespace().nth(1).unwrap(); // idk what this do
                            let url = Url::parse(&("http://localhost".to_string() + redirect_url)).unwrap();

                            let code_pair = url
                                .query_pairs()
                                .find(|pair| {
                                    let &(ref key, _) = pair;
                                    key == "code"
                                })
                                .unwrap();
    
                            let (_, value) = code_pair;
                            state.0 = AuthorizationCode::new(value.into_owned());
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
                    }
                    Err(e) => {
                        println!("ERR: {:?}",e);
                    }
                }

                state.2 = authorize_url.into();

                state
            })).then(asyn!(state, mut commands: Commands=>{

                // sends some signal to user to allow display of login link (???)
                // watch for change on Res<BevyFirebaseAuthorizeUrl> ?
                // puts auth url in resource
                commands.insert_resource(BevyFirebaseAuthorizeUrl(Url::from_str(&*state.2).unwrap()));

                // Exchange the code with a token.
                let google_token = state.1
                    .exchange_code(state.0.clone())
                    .set_pkce_verifier(PkceCodeVerifier::new(state.3.secret().into()))
                    .request(http_client);

                state.2 = google_token.as_ref().unwrap().access_token().secret().into();

                state

                // TODO exchange google token for firebase token
            }))
            .then(asyn!(state,
                firebase_api_key: Res<BevyFirebaseApiKey>,
                port: Res<BevyFirebaseRedirectPort> => {
                println!("T-then-2\n");

                    let google_access_token = state.2.clone();

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
                        google_access_token, "google.com", port.0
                    ))
                    .send()
            }))
            .then(asyn!(_state, result, mut commands:Commands => {
                println!("T-then-3\n");

                let json = serde_json::from_str::<serde_json::Value>(result.unwrap().text().unwrap()).unwrap();

                let id_token = json.get("idToken").unwrap().as_str().unwrap();

                commands.insert_resource(BevyFirebaseIdToken(id_token.into()));
            }))
        );

        println!("T-init-end\n");
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
}
