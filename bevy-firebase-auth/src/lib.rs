use std::{
    collections::HashMap,
    fs::{create_dir_all, remove_file, write, File},
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
};

use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use bevy::prelude::*;

use bevy_tokio_tasks::TokioTasksRuntime;

use dirs::cache_dir;

use ron::de::from_reader;

// Sign In Methods
// app id, client id, application id, and twitter's api key are all client_id
// app secret, client secret, application secret, and twitter's api secret are all client_secret
#[derive(Clone, Eq, PartialEq, Hash, Debug, Deserialize)]
pub enum LoginProvider {
    Google,
    Github,
    // NOT YET IMPLEMENTED
    EmailPassword,
    Apple,
    Phone,
    Anonymous,
    GooglePlayGames,
    AppleGameCenter,
    Facebook,
    Twitter,
    Microsoft,
    Yahoo,
}

/// e.g.
/// ```
/// # use bevy::prelude::*;
/// # use bevy_firebase_auth::*;
/// # use std::collections::HashMap;
/// let mut map: LoginKeysMap = HashMap::new();
/// map.insert(LoginProvider::Google, Some(("client_id".into(), "client_secret".into())));
pub type LoginKeysMap = HashMap<LoginProvider, Option<(String, String)>>;
pub type AuthUrlsMap = HashMap<LoginProvider, Url>;
pub type AuthCodesMap = HashMap<LoginProvider, String>;

#[derive(Resource)]
struct LoginKeys(LoginKeysMap);

/// Event that is sent when an Authorization URL is created
///
/// # Examples
///
/// Consuming the event:
/// ```
/// # use bevy::prelude::*;
/// # use bevy_firebase_auth::*;
/// fn auth_url_listener(
///     mut er: EventReader<AuthUrlsEvent>,
/// ) {
///     for e in er.iter() {
///         for (provider, auth_url) in e.0.iter() {
///             let mut provider_name = "";
///             let mut display_url = "";
///             match provider {
///                 LoginProvider::Google => {
///                     provider_name = "Google";
///                     display_url = auth_url.as_str();
///                 }
///                 LoginProvider::Github => {
///                     provider_name = "Github";
///                     display_url = auth_url.as_str();
///                 }
///                 _ => (),
///             }
///
///             println!(
///                 "Go to this URL to sign in with {}:\n{}\n",
///                 provider_name, display_url
///             );
///         }
///     }
/// }
#[derive(Event, Debug)]
pub struct AuthUrlsEvent(pub AuthUrlsMap);

#[derive(Event, Debug)]
pub struct AuthCodeEvent((LoginProvider, String));

#[derive(Event, Resource)]
pub struct SelectedProvider(pub LoginProvider);

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

#[derive(Resource)]
pub struct RememberLoginFlag(pub bool);

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
/// # use bevy::prelude::*;
/// # use bevy_firebase_auth::*;
/// # let mut app = App::new();
/// app.add_plugins(bevy_firebase_auth::AuthPlugin {
///     firebase_project_id: "YOUR-PROJECT-ID".into(),
///     ..Default::default()
/// });
/// ```
pub struct AuthPlugin {
    pub firebase_api_key: String,
    pub firebase_project_id: String,
    pub login_keys: LoginKeysMap,
    /// "http://127.0.0.1:9099"
    pub emulator_url: Option<String>,
}

impl Default for AuthPlugin {
    fn default() -> Self {
        let keys_path = "keys.ron";
        let f = File::open(&keys_path);

        let login_keys = match f {
            Ok(f) => {
                let login_keys: LoginKeysMap = match from_reader(f) {
                    Ok(keys) => keys,
                    Err(err) => {
                        println!("File read error: {:?}", err);
                        HashMap::new()
                    }
                };
                login_keys
            }
            Err(err) => {
                println!("File open error: {:?}", err);
                HashMap::new()
            }
        };

        AuthPlugin {
            firebase_api_key: "API_KEY".into(),
            firebase_project_id: "demo-bevy".into(),
            emulator_url: Some("http://127.0.0.1:9099".into()),
            login_keys,
        }
    }
}

impl Plugin for AuthPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ApiKey(self.firebase_api_key.clone()))
            .insert_resource(ProjectId(self.firebase_project_id.clone()))
            .insert_resource(TokenData::default())
            .insert_resource(LoginKeys(self.login_keys.clone()))
            .insert_resource(RememberLoginFlag(false))
            .add_state::<AuthState>()
            .add_event::<AuthUrlsEvent>()
            .add_event::<AuthCodeEvent>()
            .add_systems(OnEnter(AuthState::LogIn), init_login)
            .add_systems(OnEnter(AuthState::GotAuthCode), auth_code_to_firebase_token)
            .add_systems(OnEnter(AuthState::Refreshing), refresh_login)
            .add_systems(OnEnter(AuthState::LoggedIn), save_refresh_token)
            .add_systems(OnEnter(AuthState::LoggedIn), login_clear_resources)
            .add_systems(OnEnter(AuthState::LogOut), logout_clear_resources);

        // check for existing token

        let path = cache_dir()
            .clone()
            .unwrap()
            .join(std::env::var("CARGO_PKG_NAME").unwrap())
            .join("login")
            .join("firebase-refresh.key");

        let token = std::fs::read_to_string(path);

        match token {
            Ok(token) => {
                app.insert_resource(TokenData {
                    refresh_token: token,
                    ..Default::default()
                });
            }
            Err(_) => {}
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
/// # use bevy::prelude::*;
/// # use bevy_firebase_auth::*;
/// # let mut app = App::new();
/// #[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
/// enum AppAuthState {
///     #[default]
///     LogIn,
///     LogOut,
///     Delete
/// };
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
/// # use bevy::prelude::*;
/// # use bevy_firebase_auth::*;
/// # let mut app = App::new();
/// #[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
/// enum AppAuthState {
///     #[default]
///     LogIn,
///     LogOut,
///     Delete
/// };
/// app.add_state::<AppAuthState>()
/// .add_systems(OnEnter(AppAuthState::LogOut), log_out);
/// ```
pub fn log_out(current_state: Res<State<AuthState>>, mut next_state: ResMut<NextState<AuthState>>) {
    if *current_state.get() == AuthState::LoggedOut {
        return;
    }

    next_state.set(AuthState::LogOut);
}

fn logout_clear_resources(mut commands: Commands, mut next_state: ResMut<NextState<AuthState>>) {
    commands.remove_resource::<TokenData>();

    let path = cache_dir()
        .clone()
        .unwrap()
        .join(std::env::var("CARGO_PKG_NAME").unwrap())
        .join("login")
        .join("firebase-refresh.key");
    let _ = remove_file(path);

    next_state.set(AuthState::LoggedOut);

    println!("Logged out.");
}

fn login_clear_resources(mut commands: Commands) {
    commands.remove_resource::<RedirectPort>();
    commands.remove_resource::<GoogleToken>();
}

fn init_login(
    mut commands: Commands,
    login_keys: Res<LoginKeys>,
    mut ew: EventWriter<AuthUrlsEvent>,
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

    let mut auth_urls = HashMap::new();

    for (provider, optional_keys) in login_keys.0.iter() {
        let mut client_id = String::new();
        if let Some(keys) = optional_keys {
            client_id = keys.0.clone();
        }
        match provider {
            LoginProvider::Google => {
                let google_url = Url::parse(&format!("https://accounts.google.com/o/oauth2/v2/auth?scope=openid profile email&response_type=code&redirect_uri=http://127.0.0.1:{}&client_id={}",port, client_id)).unwrap();
                auth_urls.insert(LoginProvider::Google, google_url);
            }
            LoginProvider::Github => {
                let github_url: Url = Url::parse(&format!("https://github.com/login/oauth/authorize?scope=read:user&redirect_uri=http://127.0.0.1:{}&client_id={}", port, client_id )).unwrap();
                auth_urls.insert(LoginProvider::Github, github_url);
            }
            unknown_provider => {
                panic!("NOT IMPLEMENTED! {:?}", unknown_provider);
            }
        }
    }

    ew.send(AuthUrlsEvent(auth_urls));

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
                                // Grab provider flag resource from world
                                let selected_provider =
                                    ctx.world.get_resource::<SelectedProvider>();

                                if let Some(selected_provider) = selected_provider {
                                    // Match on provider flag
                                    match selected_provider.0.clone() {
                                        LoginProvider::Google => {
                                            ctx.world.send_event(AuthCodeEvent((
                                                LoginProvider::Google,
                                                code,
                                            )));
                                        }
                                        LoginProvider::Github => {
                                            ctx.world.send_event(AuthCodeEvent((
                                                LoginProvider::Github,
                                                code,
                                            )));
                                        }
                                        _ => panic!("NO SELECTED PROVIDER"),
                                    }
                                }

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
    mut auth_code_event_reader: EventReader<AuthCodeEvent>,
    runtime: ResMut<TokioTasksRuntime>,
    port: Res<RedirectPort>,
    api_key: Res<ApiKey>,
    emulator: Option<Res<AuthEmulatorUrl>>,
    login_keys: Res<LoginKeys>,
) {
    let root_url = match emulator {
        Some(url) => format!("{}/identitytoolkit.googleapis.com", url.0.clone()),
        None => "https://identitytoolkit.googleapis.com".into(),
    };

    for auth_code_event in auth_code_event_reader.iter() {
        let (provider, auth_code) = auth_code_event.0.clone();

        if let Some(keys) = login_keys.0.get(&provider) {
            if let Some((client_id, client_secret)) = keys {
                let api_key = api_key.0.clone();
                let port = format!("{}", port.0);
                let auth_code = auth_code.clone();
                let root_url = root_url.clone();
                let client_secret = client_secret.clone();
                let client_id = client_id.clone();
                let provider = provider.clone();

                runtime.spawn_background_task(|mut ctx| async move {
                let client = reqwest::Client::new();
                let mut body: HashMap<String, Value> = HashMap::new();

                match provider.clone() {
                    LoginProvider::Google => {
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

                        body.insert(
                            "postBody".into(),
                            Value::String(format!(
                                "id_token={}&providerId={}",
                                id_token, "google.com"
                            )),
                        );
                    }
                    LoginProvider::Github => {
                        // TODO no github on emulator

                        #[derive(Deserialize, Debug)]
                        struct GithubTokenResponse {
                            access_token: String
                        }

                        let response = client.post(format!("https://github.com/login/oauth/access_token?client_id={}&client_secret={}&code={}",client_id,client_secret,auth_code))
                        .header("Accept", "application/json")
                        .send()
                        .await
                        .unwrap()
                        .json::<GithubTokenResponse>()
                        .await
                        .unwrap();

                        let access_token = response.access_token;

                        body.insert(
                            "postBody".into(), 
                            Value::String(format!(
                                "access_token={}&providerId={}",
                                access_token, "github.com"
                            ))
                        );
                    }
                    _ => (),
                }

                // Add common params
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

                ctx.run_on_main_thread(move |ctx| {
                    ctx.world.insert_resource(firebase_token);

                    // Set next state
                    ctx.world
                        .insert_resource(NextState(Some(AuthState::LoggedIn)));
                })
                .await;
            });
            }
        }
    }
}

fn save_refresh_token(token_data: Res<TokenData>, remember_login: Res<RememberLoginFlag>) {
    if !remember_login.0 {
        return;
    }

    let path = cache_dir()
        .unwrap()
        .join(std::env::var("CARGO_PKG_NAME").unwrap())
        .join("login");

    let dir_result = create_dir_all(path.clone());

    match dir_result {
        Ok(()) => {}
        Err(err) => println!("Couldn't create login directory: {:?}", err),
    }

    let save_result = write(
        path.clone().join("firebase-refresh.key"),
        token_data.refresh_token.as_str(),
    );

    match save_result {
        Ok(()) => {}
        Err(err) => println!("Couldn't save refresh token to {:?}: {:?}", path, err),
    }
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
            .await;

        let firebase_token = match firebase_token {
            Ok(token) => token,
            Err(_) => {
                // Set state to logout on failure
                ctx.run_on_main_thread(|ctx| {
                    ctx.world.insert_resource(NextState(Some(AuthState::LogIn)))
                })
                .await;
                return;
            }
        };

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
/// # use bevy::prelude::*;
/// # use bevy_firebase_auth::*;
/// # let mut app = App::new();
/// #[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
/// enum AppAuthState {
///     #[default]
///     LogIn,
///     LogOut,
///     Delete
/// };
/// app.add_systems(OnEnter(AppAuthState::Delete), delete_account);
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

        ctx.run_on_main_thread(move |ctx| {
            // Set next state
            ctx.world
                .insert_resource(NextState(Some(AuthState::LogOut)));
        })
        .await;
    });
}
