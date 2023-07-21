use std::{
    collections::HashMap,
    fs::{read_to_string, remove_file, write},
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
    path::PathBuf,
};

use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use bevy::prelude::*;

use bevy_tokio_tasks::TokioTasksRuntime;

// Sign In Methods
// app id, client id, application id, and twitter's api key are all client_id
// app secret, client secret, application secret, and twitter's api secret are all client_secret
#[derive(Clone)]
pub enum SignInMethod {
    Google {
        client_id: String,
        client_secret: String,
    },
    GitHub {
        client_id: String,
        client_secret: String,
    },
    EmailPassword,
    // TODO
    Apple,
    Phone,
    Anonymous,
    GooglePlayGames {
        client_id: String,
        client_secret: String,
    },
    Facebook {
        client_id: String,
        client_secret: String,
    },
    Twitter {
        client_id: String,
        client_secret: String,
    },
    Microsoft {
        client_id: String,
        client_secret: String,
    },
    Yahoo {
        client_id: String,
        client_secret: String,
    },
}

#[derive(Resource, Clone)]
pub struct SignInMethods(Vec<SignInMethod>);

#[derive(Debug)]
pub enum AuthUrl {
    Google(Url),
    GitHub(Url),
    Facebook(Url),
    Twitter(Url),
    Microsoft(Url),
    Yahoo(Url),
}
/// Event that is sent when an Authorization URL is created
///
/// # Examples
///
/// Consuming the event:
/// ```
/// fn auth_url_listener(mut er: EventReader<GotAuthUrl>) {
///     for e in er.iter() {
///         println!("Go to this URL to sign in:\n{}\n", e.0);
///     }
/// }
#[derive(Event, Debug)]
pub struct AuthUrls(pub Vec<AuthUrl>);

#[derive(Resource, Clone)]
pub struct AuthEmulatorUrl(String);

// From plugin
/// Bevy `Resource` containing the app's Firebase API key
#[derive(Resource)]
pub struct ApiKey(String);

// From plugin
/// Bevy `Resource` containing the app's Firebase Project ID
#[derive(Resource)]
pub struct ProjectId(pub String);

// TODO trim this down?
/// Holds data from a user access token
#[derive(Deserialize, Resource, Default, Debug)]
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

/// The status of the held access token
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

/// The Firebase Auth bevy plugin
///
/// # Examples
///
/// Usage:
/// ```
/// app.add_plugins(bevy_firebase_auth::AuthPlugin {
///     firebase_project_id: "YOUR-PROJECT-ID".into(),
///     ..Default::default()
/// });
/// ```
/// This retrieves keys saved in data/keys/
/// - firebase-api.key
/// - google-client-id.key
/// - google-client-secret.key
/// - firebase-refresh.key (OPTIONAL)
///
pub struct AuthPlugin {
    pub firebase_api_key: String,
    pub firebase_project_id: String,
    pub firebase_refresh_token: Option<String>,
    pub sign_in_methods: SignInMethods,
    /// "http://127.0.0.1:9099"
    pub emulator_url: Option<String>,
}

impl Default for AuthPlugin {
    fn default() -> Self {
        let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);

        let firebase_refresh_token =
            match read_to_string(data_dir.join("keys/firebase-refresh.key")) {
                Ok(key) => Some(key),
                Err(_) => None,
            };

        let mut sign_in_methods = Vec::new();

        let google_client_id = match read_to_string(data_dir.join("keys/google-client-id.key")) {
            Ok(key) => Some(key),
            Err(_) => None,
        };

        let google_client_secret =
            match read_to_string(data_dir.join("keys/google-client-secret.key")) {
                Ok(key) => Some(key),
                Err(_) => None,
            };

        if let (Some(client_id), Some(client_secret)) = (google_client_id, google_client_secret) {
            sign_in_methods.push(SignInMethod::Google {
                client_id,
                client_secret,
            })
        }

        // TODO reenable this code when differentiation between oAuth code receiving works
        // let github_client_id = match read_to_string(data_dir.join("keys/github-client-id.key")) {
        //     Ok(key) => Some(key),
        //     Err(_) => None,
        // };

        // let github_client_secret =
        //     match read_to_string(data_dir.join("keys/github-client-secret.key")) {
        //         Ok(key) => Some(key),
        //         Err(_) => None,
        //     };

        // if let (Some(client_id), Some(client_secret)) = (github_client_id, github_client_secret) {
        //     sign_in_methods.push(SignInMethod::GitHub {
        //         client_id,
        //         client_secret,
        //     })
        // }

        AuthPlugin {
            firebase_api_key: "literally anything for emulator".into(),
            firebase_refresh_token,
            firebase_project_id: "demo-bevy".into(),
            emulator_url: Some("http://127.0.0.1:9099".into()),
            sign_in_methods: SignInMethods(sign_in_methods),
        }
    }
}

impl Plugin for AuthPlugin {
    fn build(&self, app: &mut App) {
        // TODO optionally save refresh token to file

        app.insert_resource(ApiKey(self.firebase_api_key.clone()))
            .insert_resource(ProjectId(self.firebase_project_id.clone()))
            .insert_resource(TokenData::default())
            .insert_resource(self.sign_in_methods.clone())
            .add_state::<AuthState>()
            .add_event::<AuthUrls>()
            .add_systems(OnEnter(AuthState::LogIn), init_login)
            .add_systems(
                OnEnter(AuthState::GotAuthCode),
                google_id_token_to_firebase_token,
            )
            .add_systems(OnEnter(AuthState::Refreshing), refresh_login)
            .add_systems(OnEnter(AuthState::LoggedIn), save_refresh_token)
            .add_systems(OnEnter(AuthState::LogOut), login_clear_resources)
            .add_systems(OnEnter(AuthState::LogOut), logout_clear_resources);

        if self.firebase_refresh_token.is_some() {
            app.insert_resource(TokenData {
                refresh_token: self.firebase_refresh_token.clone().unwrap(),
                ..Default::default()
            });
        }

        if self.emulator_url.is_some() {
            app.insert_resource(AuthEmulatorUrl(self.emulator_url.clone().unwrap()));
        }
    }
}

// designed to be called on user managed state change
// but ofc can be passed params
/// Function to log in
///
/// Designed to be called on a user managed state change.
///
/// # Examples
///
/// ```
/// app.add_state::<AppAuthState>()
/// .add_systems(OnEnter(AppAuthState::LogIn), log_in);
pub fn log_in(
    current_state: Res<State<AuthState>>,
    mut next_state: ResMut<NextState<AuthState>>,
    token_data: Option<Res<TokenData>>,
) {
    if *current_state.get() != AuthState::LoggedOut {
        return;
    }

    if token_data.is_none() || token_data.unwrap().refresh_token.clone().is_empty() {
        next_state.set(AuthState::LogIn);
    } else {
        next_state.set(AuthState::Refreshing);
    }
}

/// Function to log out
///
/// Designed to be called on a user managed state change.
///
/// # Examples
///
/// ```
/// app.add_state::<AppAuthState>()
/// .add_systems(OnEnter(AppAuthState::LogOut) log_out);
/// ```
pub fn log_out(current_state: Res<State<AuthState>>, mut next_state: ResMut<NextState<AuthState>>) {
    if *current_state.get() == AuthState::LoggedOut {
        return;
    }

    next_state.set(AuthState::LogOut);
}

fn logout_clear_resources(mut commands: Commands, mut next_state: ResMut<NextState<AuthState>>) {
    commands.remove_resource::<TokenData>();

    let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
    let _ = remove_file(data_dir.join("keys/firebase-refresh.key"));

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
    sign_in_methods: Res<SignInMethods>,
    mut ew: EventWriter<AuthUrls>,
    runtime: ResMut<TokioTasksRuntime>,
) {
    // sets up redirect server
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();

    match listener.set_nonblocking(true) {
        Ok(_) => {}
        Err(err) => println!(
            "Couldn't set nonblocking listener! This may cause an app freeze on exit. {:?}",
            err
        ),
    };

    let port = listener.local_addr().unwrap().port();

    commands.insert_resource(RedirectPort(port));

    let mut auth_urls = Vec::new();

    for keys in sign_in_methods.0.iter() {
        match keys {
            SignInMethod::Google {
                client_id,
                client_secret: _,
            } => {
                let google_url = Url::parse(&format!("https://accounts.google.com/o/oauth2/v2/auth?scope=openid profile email&response_type=code&redirect_uri=http://127.0.0.1:{}&client_id={}",port, client_id)).unwrap();
                auth_urls.push(AuthUrl::Google(google_url));
            }
            SignInMethod::GitHub {
                client_id,
                client_secret: _,
            } => {
                let github_url = Url::parse(&format!("https://github.com/login/oauth/authorize?scope=user:email&redirect_uri=http://127.0.0.1:{}&client_id={}", port, client_id )).unwrap();
                auth_urls.push(AuthUrl::GitHub(github_url));
            }
            SignInMethod::EmailPassword => {}
            _ => {}
        }
    }

    ew.send(AuthUrls(auth_urls));

    // TODO figure out differentiating between providers
    // Server needs to be spun up and ready before giving user auth URLs
    //  cos the login can be pretty quick
    // BUT the stream has no data on where it came from
    // SO there needs to be another way to discern what oAuth things we're dealing with.
    // MAYBE when user clicks the provider button set the server up?
    // Nah, wouldn't work in console.
    // Set a flag when the user clicks a login button?
    // But what if they click all of them? Then there would have to be button disabling, and timeout if that login fails....
    // IF all providers return their code as a `code` param I could sort on the other side?
    // Only by trying and failing though, that doesn't seem right.
    // Spawn a server for each provider type? More unnecessary computation locally but could work?
    // EventReader for AuthUrls, only spawn listeners for URLs we have.

    runtime.spawn_background_task(|mut ctx| async move {
        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    {
                        let mut reader = BufReader::new(&stream);
                        let mut request_line = String::new();
                        reader.read_line(&mut request_line).unwrap(); // first line of stream is like GET /?code=blahBlBlAh&otherStuff=1 HTTP/1.1

                        let redirect_url = request_line.split_whitespace().nth(1).unwrap(); // gets second part of first line of stream, so the path & params
                        let url = Url::parse(&("http://localhost".to_string() + redirect_url)); // reconstructs a valid URL

                        let url = url.unwrap().to_owned();

                        // gets the `code` param from reconstructed url
                        let code_pair = url.query_pairs().find(|pair| {
                            let (key, _) = pair;
                            key == "code"
                        });

                        if let Some(code_pair) = code_pair {
                            let code = code_pair.1.into_owned();
                            ctx.run_on_main_thread(move |ctx| {
                                // TODO differentiate between providers here or before
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

fn google_id_token_to_firebase_token(
    auth_code: Res<GoogleAuthCode>,
    runtime: ResMut<TokioTasksRuntime>,
    port: Res<RedirectPort>,
    api_key: Res<ApiKey>,
    emulator: Option<Res<AuthEmulatorUrl>>,
    sign_in_methods: Res<SignInMethods>,
) {
    for keys in sign_in_methods.0.iter() {
        if let SignInMethod::Google {
            client_id,
            client_secret,
        } = keys.clone()
        {
            // do stuff
            let api_key = api_key.0.clone();

            let auth_code = auth_code.0.clone();
            let port = format!("{}", port.0);

            let root_url = match emulator {
                Some(url) => format!("{}/identitytoolkit.googleapis.com", url.0.clone()),
                None => "https://identitytoolkit.googleapis.com".into(),
            };

            runtime.spawn_background_task(|mut ctx| async move {
                let client = reqwest::Client::new();
                let form = reqwest::multipart::Form::new()
                    .text("code", auth_code)
                    .text("client_id", client_id)
                    .text("client_secret", client_secret)
                    .text("redirect_uri", format!("http://127.0.0.1:{port}"))
                    .text("grant_type", "authorization_code");

                #[derive(Deserialize, Debug)]
                struct GoogleTokenResponse {
                    id_token: String,
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

                let id_token = google_token.id_token;

                let mut body: HashMap<String, Value> = HashMap::new();
                body.insert(
                    "postBody".into(),
                    Value::String(format!("id_token={}&providerId={}", id_token, "google.com")),
                );
                body.insert(
                    "requestUri".into(),
                    Value::String(format!("http://127.0.0.1:{port}")),
                );
                body.insert("returnIdpCredential".into(), true.into());
                body.insert("returnSecureToken".into(), true.into());

                // Get Firebase Token
                let firebase_token = client
                    .post(format!(
                        "{}/v1/accounts:signInWithIdp?key={}",
                        root_url, api_key
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

            break;
        }
    }
}

fn save_refresh_token(token_data: Res<TokenData>) {
    let data_dir = PathBuf::from_iter([std::env!("CARGO_MANIFEST_DIR"), "data"]);
    let _ = write(
        data_dir.join("keys/firebase-refresh.key"),
        token_data.refresh_token.as_str(),
    );
}

fn refresh_login(
    token_data: Res<TokenData>,
    firebase_api_key: Res<ApiKey>,
    runtime: ResMut<TokioTasksRuntime>,
    emulator: Option<Res<AuthEmulatorUrl>>,
) {
    let refresh_token = token_data.refresh_token.clone();
    let api_key = firebase_api_key.0.clone();
    let root_url = match emulator {
        Some(url) => format!("{}/securetoken.googleapis.com", url.0),
        None => "https://securetoken.googleapis.com".into(),
    };

    runtime.spawn_background_task(|mut ctx| async move {
        let client = Client::new();

        let firebase_token = client
            .post(format!("{}/v1/token?key={}", root_url, api_key))
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

        // TODO handle errors here, panic prevents login button being generated
        // TODO if login fails here, delete saved refresh key and break

        // Use Firebase Token
        ctx.run_on_main_thread(move |ctx| {
            ctx.world.insert_resource(firebase_token);

            // Set next state
            ctx.world
                .insert_resource(NextState(Some(AuthState::LoggedIn)));
        })
        .await;
    });
}

/// Function to delete an account from Firebase
///
/// To be triggered with on state change
///
/// # Examples
///
/// Usage:
/// ```
/// app.add_systems(OnEnter(AppAuthState::Delete), delete_account)
pub fn delete_account(
    token_data: Res<TokenData>,
    firebase_api_key: Res<ApiKey>,
    runtime: ResMut<TokioTasksRuntime>,
    emulator: Option<Res<AuthEmulatorUrl>>,
) {
    let api_key = firebase_api_key.0.clone();
    let id_token = token_data.id_token.clone();
    let root_url = match emulator {
        Some(url) => format!("{}/identitytoolkit.googleapis.com", url.0),
        None => "https://identitytoolkit.googleapis.com".into(),
    };
    runtime.spawn_background_task(|mut ctx| async move {
        let client = Client::new();
        let mut body = HashMap::new();
        body.insert("idToken", id_token);

        let _res = client
            .post(format!("{}/v1/accounts:delete?key={}", root_url, api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .unwrap()
            .text()
            .await;

        // TODO handle errors like CREDENTIAL_TOO_OLD

        // Use Firebase Token TODO pull into fn?
        ctx.run_on_main_thread(move |ctx| {
            // Set next state
            ctx.world
                .insert_resource(NextState(Some(AuthState::LogOut)));
        })
        .await;
    });
}
