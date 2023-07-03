use std::{
    collections::HashMap,
    fs::{read_to_string, remove_file, write},
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
    path::PathBuf,
};

use reqwest::Client;
use serde::Deserialize;
use url::Url;

use bevy::prelude::*;

use bevy_tokio_tasks::TokioTasksRuntime;

// AUTH

// From plugin
#[derive(Resource)]
struct GoogleClientId(String);

// From plugin
#[derive(Resource)]
struct GoogleClientSecret(String);

// From plugin
#[derive(Resource)]
pub struct ApiKey(String);

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
        let firebase_api_key = read_to_string(data_dir.join("keys/firebase-api.key")).unwrap();
        let google_client_id = read_to_string(data_dir.join("keys/google-client-id.key")).unwrap();
        let google_client_secret =
            read_to_string(data_dir.join("keys/google-client-secret.key")).unwrap();
        let firebase_refresh_token = read_to_string(data_dir.join("keys/firebase-refresh.key"));

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
    token_data: Option<Res<TokenData>>,
) {
    if current_state.0 != AuthState::LoggedOut {
        return;
    }

    if token_data.is_none() || token_data.unwrap().refresh_token.clone().is_empty() {
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
    google_client_id: Res<GoogleClientId>,
    mut ew: EventWriter<GotAuthUrl>,
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
        data_dir.join("keys/firebase-refresh.key"),
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
        let client = Client::new();

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

        // TODO handle errors here, panic prevents login button being generated

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

pub fn delete_account(
    token_data: Res<TokenData>,
    firebase_api_key: Res<ApiKey>,
    runtime: ResMut<TokioTasksRuntime>,
) {
    let api_key = firebase_api_key.0.clone();
    let id_token = token_data.id_token.clone();

    runtime.spawn_background_task(|mut ctx| async move {
        let client = Client::new();
        let mut body = HashMap::new();
        body.insert("idToken", id_token);

        let res = client
            .post(format!(
                "https://identitytoolkit.googleapis.com/v1/accounts:delete?key={}",
                api_key
            ))
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
