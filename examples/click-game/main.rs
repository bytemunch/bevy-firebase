// CLICK
// click button, add to score
// has login
// has online leaderboard

mod util;

use std::collections::HashMap;

use bevy::{app::AppExit, prelude::*};
use bevy_firebase_auth::{
    delete_account, log_in, log_out, AuthState, GotAuthUrl, ProjectId, TokenData,
};
use bevy_firebase_firestore::{
    async_delete_document, async_read_document, async_update_document, value::ValueType,
    BevyFirestoreClient, Document, DocumentMask, FirestoreState, QueryDirection,
    QueryResponseEvent, RunQueryEvent, RunQueryResponse, Status, UpdateDocumentEvent,
    UpdateDocumentRequest, Value,
};
use bevy_tokio_tasks::TokioTasksRuntime;
use util::despawn_with;

// colours
const NORMAL_BUTTON: Color = Color::rgb(0.15, 0.15, 0.15);
const HOVERED_BUTTON: Color = Color::rgb(0.25, 0.25, 0.25);
const PRESSED_BUTTON: Color = Color::rgb(0.35, 0.75, 0.35);
const TEXT_COLOR: Color = Color::rgb(0.9, 0.9, 0.9);

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
enum AuthControllerState {
    #[default]
    Start,
    LogIn,
    LogOut,
    Delete,
}

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
enum AppScreenState {
    #[default]
    LogInScreen,
    MainMenu,
    InGame,
    Leaderboard,
}

#[derive(Component)]
struct LogInScreenData;

#[derive(Component)]
struct MainMenuData;

#[derive(Component)]
struct InGameData;

#[derive(Component)]
struct LeaderboardData;

fn main() {
    App::new()
        // PLUGINS
        .add_plugins(DefaultPlugins)
        .add_plugin(bevy_firebase_auth::AuthPlugin {
            firebase_project_id: "test-auth-rs".into(),
            ..Default::default()
        })
        .add_plugin(bevy_firebase_firestore::FirestorePlugin {
            emulator_url: Some("http://127.0.0.1:8080".into()),
            // emulator_url: None,
        })
        .add_plugin(bevy_tokio_tasks::TokioTasksPlugin::default())
        // STATES
        .add_state::<AuthControllerState>()
        .add_state::<AppScreenState>()
        // INIT
        .add_startup_system(setup)
        // UTILS
        .add_system(button_color_system)
        .add_system(exit_button_system)
        // LOGIN
        .add_system(log_in.in_schedule(OnEnter(AuthControllerState::LogIn)))
        .add_system(log_out.in_schedule(OnEnter(AuthControllerState::LogOut)))
        .add_system(delete_account.in_schedule(OnEnter(AuthControllerState::Delete)))
        .add_system(logged_in.in_schedule(OnEnter(AuthState::LoggedIn)))
        .add_system(firestore_ready.in_schedule(OnEnter(FirestoreState::Ready)))
        .add_system(logged_out.in_schedule(OnEnter(AuthState::LoggedOut)))
        // SCREENS
        // login
        .add_system(build_login_screen.in_schedule(OnEnter(AppScreenState::LogInScreen)))
        .add_system(
            despawn_with::<LogInScreenData>.in_schedule(OnExit(AppScreenState::LogInScreen)),
        )
        .add_system(login_button_system.in_set(OnUpdate(AppScreenState::LogInScreen)))
        .add_system(auth_url_listener.in_set(OnUpdate(AuthControllerState::LogIn)))
        // menu
        .add_system(build_main_menu.in_schedule(OnEnter(AppScreenState::MainMenu)))
        .add_system(despawn_with::<MainMenuData>.in_schedule(OnExit(AppScreenState::MainMenu)))
        .add_system(play_button_system.in_set(OnUpdate(AppScreenState::MainMenu)))
        .add_system(nickname_submit_button_system.in_set(OnUpdate(AppScreenState::MainMenu)))
        .add_system(delete_score_button_system.in_set(OnUpdate(AppScreenState::MainMenu)))
        .add_system(delete_account_button_system.in_set(OnUpdate(AppScreenState::MainMenu)))
        .add_system(logout_button_system.in_set(OnUpdate(AppScreenState::MainMenu)))
        .add_system(leaderboard_button_system.in_set(OnUpdate(AppScreenState::MainMenu)))
        .add_system(update_welcome_text.in_set(OnUpdate(AppScreenState::MainMenu)))
        // in game
        .add_system(build_in_game.in_schedule(OnEnter(AppScreenState::InGame)))
        .add_system(despawn_with::<InGameData>.in_schedule(OnExit(AppScreenState::InGame)))
        .add_system(update_score.in_set(OnUpdate(AppScreenState::InGame)))
        .add_system(score_button_system.in_set(OnUpdate(AppScreenState::InGame)))
        .add_system(return_to_menu_button_system.in_set(OnUpdate(AppScreenState::InGame)))
        .add_system(submit_score_button_system.in_set(OnUpdate(AppScreenState::InGame)))
        // leaderboard
        .add_event::<UpdateLeaderboardEvent>()
        .add_system(build_leaderboard.in_schedule(OnEnter(AppScreenState::Leaderboard)))
        .add_system(
            despawn_with::<LeaderboardData>.in_schedule(OnExit(AppScreenState::Leaderboard)),
        )
        .add_system(query_response_event_handler)
        .add_system(return_to_menu_button_system.in_set(OnUpdate(AppScreenState::Leaderboard)))
        .add_system(update_leaderboard.in_set(OnUpdate(AppScreenState::Leaderboard)))
        .run();
}

// UI

#[derive(Resource, Clone)]
struct UiSettings {
    typefaces: TypeFaces,
    button: ButtonBundle,
}

#[derive(Clone)]
struct TypeFaces {
    h1: TextStyle,
    h2: TextStyle,
    p: TextStyle,
}

#[derive(Component)]
struct UiBase;

// LOGIN

#[derive(Component)]
struct LoginButton(String);

#[derive(Component)]
struct ExitButton;

// MENU

#[derive(Component)]
struct WelcomeText;

#[derive(Component)]
struct LogoutButton;

#[derive(Component)]
struct PlayButton;

#[derive(Component)]
struct NicknameSubmitButton;

#[derive(Component, Clone)]
struct NicknameInput {
    value: String,
}

#[derive(Component)]
struct DeleteScoreButton;

#[derive(Component)]
struct DeleteAccountButton;

#[derive(Component)]
struct LeaderboardButton;

// IN GAME

#[derive(Component)]
struct ScoreButton;

#[derive(Component)]
struct SubmitScoreButton;

#[derive(Component)]
struct ReturnToMenuButton;

#[derive(Component)]
struct ScoreText;

// LEADERBOARD
#[derive(Component)]
struct Leaderboard;

// GAME LOGIC
#[derive(Resource)]
struct Score(i64);

#[derive(Resource)]
struct Nickname(String);

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2dBundle::default());

    // SETUP UI
    let font = asset_server.load("fonts/HackNerdFont-Regular.ttf");

    let typefaces = TypeFaces {
        h1: TextStyle {
            font: font.clone(),
            font_size: 60.0,
            color: TEXT_COLOR,
        },
        h2: TextStyle {
            font: font.clone(),
            font_size: 40.0,
            color: TEXT_COLOR,
        },
        p: TextStyle {
            font,
            font_size: 20.0,
            color: TEXT_COLOR,
        },
    };

    let button = ButtonBundle {
        style: Style {
            size: Size::new(Val::Px(300.0), Val::Px(65.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            margin: UiRect {
                top: Val::Px(10.),
                ..Default::default()
            },
            ..default()
        },
        background_color: NORMAL_BUTTON.into(),
        ..Default::default()
    };

    commands
        .spawn(NodeBundle {
            style: Style {
                size: Size {
                    height: Val::Percent(100.),
                    width: Val::Percent(100.),
                },
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            ..default()
        })
        .insert(UiBase)
        .with_children(|parent| {
            parent.spawn((
                TextBundle::from_section("CLiCK", typefaces.h1.clone()).with_style(Style {
                    ..Default::default()
                }),
            ));
        });

    commands.insert_resource(UiSettings { typefaces, button });

    // This is populated on firestore load
    commands.insert_resource(Score(0));
}

// UTILS

fn button_color_system(
    mut q_interaction: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, mut color) in &mut q_interaction {
        match *interaction {
            Interaction::Clicked => {
                *color = PRESSED_BUTTON.into();
            }
            Interaction::Hovered => {
                *color = HOVERED_BUTTON.into();
            }
            Interaction::None => {
                *color = NORMAL_BUTTON.into();
            }
        }
    }
}

fn exit_button_system(
    mut q_interaction: Query<(&Interaction,), (Changed<Interaction>, With<ExitButton>)>,
    mut exit: EventWriter<AppExit>,
) {
    for (interaction,) in &mut q_interaction {
        if *interaction == Interaction::Clicked {
            exit.send(AppExit)
        }
    }
}

// LOGIN

fn build_login_screen(
    mut commands: Commands,
    mut next_state: ResMut<NextState<AuthControllerState>>,
    mut q_ui_base: Query<Entity, With<UiBase>>,
    ui: Res<UiSettings>,
) {
    println!("build_login_screen");
    let ui_base = q_ui_base.single_mut();

    commands.entity(ui_base).with_children(|parent| {
        // TITLE
        parent.spawn((
            TextBundle::from_section("login", ui.typefaces.h2.clone()).with_style(Style {
                ..Default::default()
            }),
            LogInScreenData,
        ));

        parent
            .spawn(ui.button.clone())
            .insert(ExitButton)
            .insert(LogInScreenData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section("quit", ui.typefaces.p.clone()));
            });
    });

    // attempt auto login
    next_state.set(AuthControllerState::LogIn);
}

fn login_button_system(
    mut q_interaction: Query<
        (&Interaction, &LoginButton, &Children),
        (Changed<Interaction>, With<LoginButton>),
    >,
    mut text_query: Query<&mut Text>,
) {
    for (interaction, login_url, children) in &mut q_interaction {
        let mut text = text_query.get_mut(children[0]).unwrap();

        if *interaction == Interaction::Clicked {
            // open URL
            let _ = open::that(login_url.0.clone());
            text.sections[0].value = "waiting for browser...".into();
            // TODO display this text separately, allow users to close tab and try again
        }
    }
}

fn auth_url_listener(
    mut commands: Commands,
    mut er: EventReader<GotAuthUrl>,
    mut q_ui_base: Query<Entity, With<UiBase>>,
    ui: Res<UiSettings>,
) {
    for e in er.iter() {
        println!("Go to this URL to sign in:\n{}\n", e.0);

        // add login button
        let ui_base = q_ui_base.single_mut();

        commands.entity(ui_base).with_children(|parent| {
            parent
                .spawn(ui.button.clone())
                .insert(LoginButton(e.0.clone().into()))
                .insert(LogInScreenData)
                .with_children(|parent| {
                    parent.spawn(TextBundle::from_section(
                        "log in with google",
                        ui.typefaces.p.clone(),
                    ));
                });
        });
    }
}

fn logged_in(mut _next_state: ResMut<NextState<AppScreenState>>) {
    println!("logged_in");
    // set app state to main menu
    // _next_state.set(AppScreenState::MainMenu);
}

fn firestore_ready(
    runtime: ResMut<TokioTasksRuntime>,
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    token_data: Res<TokenData>,
) {
    println!("firestore ready!");

    let mut client = client.0.clone();
    let project_id = project_id.0.clone();
    let uid = token_data.local_id.clone();

    runtime.spawn_background_task(|mut ctx| async move {
        // Get name
        let name_res =
            async_read_document(&mut client, &project_id, &format!("click/{}", uid)).await;

        let name: String = match name_res {
            Ok(res) => {
                let doc = res.into_inner();
                if let Some(val) = doc.fields.get("nickname") {
                    if let Some(ValueType::StringValue(s)) = val.clone().value_type {
                        s
                    } else {
                        "Player".into()
                    }
                } else {
                    "Player".into()
                }
            }
            Err(_) => "Player".into(),
        };

        // Set field in firestore
        if name == "Player" {
            let mut data = HashMap::new();
            data.insert(
                "nickname".to_string(),
                Value {
                    value_type: Some(ValueType::StringValue(name.clone())),
                },
            );

            let _ = client
                .update_document(UpdateDocumentRequest {
                    document: Some(Document {
                        name: format!(
                            "projects/{project_id}/databases/(default)/documents/click/{uid}"
                        ),
                        fields: data.clone(),
                        ..Default::default()
                    }),
                    update_mask: Some(DocumentMask {
                        field_paths: vec!["nickname".into()],
                    }),
                    ..Default::default()
                })
                .await;
        }

        // Get score
        let score_res =
            async_read_document(&mut client, &project_id, &format!("click/{}", uid)).await;

        let score_res = match score_res {
            Ok(res) => {
                let doc = res.into_inner();
                if let Some(val) = doc.fields.get("score") {
                    if let Some(ValueType::IntegerValue(s)) = val.clone().value_type {
                        s
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Err(_) => 0,
        };

        ctx.run_on_main_thread(move |ctx| {
            ctx.world
                .insert_resource(NextState(Some(AppScreenState::MainMenu)));
            ctx.world.insert_resource(Nickname(name));
            ctx.world.insert_resource(Score(score_res));
        })
        .await;
    });
}

fn logged_out(mut next_state: ResMut<NextState<AppScreenState>>) {
    println!("logged_out");
    // set app state to main menu
    next_state.set(AppScreenState::LogInScreen);
}

// MENU

fn build_main_menu(
    mut commands: Commands,
    mut q_ui_base: Query<Entity, With<UiBase>>,
    ui: Res<UiSettings>,
    nickname: Res<Nickname>,
) {
    println!("build_main_menu");
    let ui_base = q_ui_base.single_mut();

    // UI
    commands.entity(ui_base).with_children(|parent| {
        // TITLE
        parent.spawn((
            TextBundle::from_section("main menu", ui.typefaces.h2.clone()),
            MainMenuData,
        ));

        // WELCOME
        let name = nickname.0.clone();

        parent.spawn((
            TextBundle::from_section(format!("Welcome, {name}!"), ui.typefaces.p.clone()),
            WelcomeText,
            MainMenuData,
        ));

        // PLAY BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(PlayButton)
            .insert(MainMenuData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section("play", ui.typefaces.p.clone()));
            });

        // TODO: NICKNAME TEXT ENTRY

        // NICKNAME SUBMIT BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(NicknameSubmitButton)
            .insert(MainMenuData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section("set name", ui.typefaces.p.clone()));
            });

        // LEADERBOARD BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(LeaderboardButton)
            .insert(MainMenuData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section(
                    "leaderboard",
                    ui.typefaces.p.clone(),
                ));
            });

        // LOGOUT BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(LogoutButton)
            .insert(MainMenuData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section("log out", ui.typefaces.p.clone()));
            });

        // DELETE SCORE BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(DeleteScoreButton)
            .insert(MainMenuData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section(
                    "delete score",
                    ui.typefaces.p.clone(),
                ));
            });

        // DELETE ACCOUNT BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(DeleteAccountButton)
            .insert(MainMenuData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section(
                    "delete account",
                    ui.typefaces.p.clone(),
                ));
            });

        // EXIT BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(ExitButton)
            .insert(MainMenuData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section("quit", ui.typefaces.p.clone()));
            });
    });

    commands.spawn_empty().insert(NicknameInput {
        value: "NewName".into(),
    }); // value lost on state change? BUG NEEDS-INVESTIGATION
}

fn update_welcome_text(
    nickname: Res<Nickname>,
    mut q_welcome_text: Query<&mut Text, With<WelcomeText>>,
) {
    if nickname.is_changed() {
        let mut text = q_welcome_text.single_mut();
        text.sections[0].value = format!("welcome, {}", nickname.0);
    }
}

fn play_button_system(
    mut q_interaction: Query<(&Interaction,), (Changed<Interaction>, With<PlayButton>)>,
    mut next_state: ResMut<NextState<AppScreenState>>,
) {
    for (interaction,) in &mut q_interaction {
        if *interaction == Interaction::Clicked {
            // Go to in game state
            next_state.set(AppScreenState::InGame)
        }
    }
}

fn nickname_submit_button_system(
    q_interaction: Query<&Interaction, (Changed<Interaction>, With<NicknameSubmitButton>)>,
    q_nickname_input: Query<&NicknameInput>,
    token_data: Option<Res<TokenData>>,
    mut nickname: ResMut<Nickname>,
    mut ew: EventWriter<UpdateDocumentEvent>,
) {
    if token_data.is_none() {
        return;
    }

    let token_data = token_data.unwrap();

    if let Ok(nickname_input) = q_nickname_input.get_single() {
        if let Ok(Interaction::Clicked) = q_interaction.get_single() {
            nickname.0 = nickname_input.value.clone();

            let uid = token_data.local_id.clone();
            let nickname = nickname_input.clone();

            let mut document_data = HashMap::new();
            document_data.insert(
                "nickname".to_string(),
                Value {
                    value_type: Some(ValueType::StringValue(nickname.value)),
                },
            );

            let document_path = format!("click/{uid}");

            ew.send(UpdateDocumentEvent {
                document_path,
                document_data,
                id: 0,
            });

            // TODO nickname update listener
        }
    }
}

fn leaderboard_button_system(
    mut q_interaction: Query<(&Interaction,), (Changed<Interaction>, With<LeaderboardButton>)>,
    mut next_state: ResMut<NextState<AppScreenState>>,
) {
    for (interaction,) in &mut q_interaction {
        if *interaction == Interaction::Clicked {
            next_state.set(AppScreenState::Leaderboard)
        }
    }
}

fn delete_score_button_system(
    mut q_interaction: Query<(&Interaction,), (Changed<Interaction>, With<DeleteScoreButton>)>,
    mut score: ResMut<Score>,
    token_data: Option<Res<TokenData>>,
    mut ew: EventWriter<UpdateDocumentEvent>,
) {
    // TODO early return, tooooo much right shift
    if let Some(token_data) = token_data {
        for (interaction,) in &mut q_interaction {
            if *interaction == Interaction::Clicked {
                score.0 = 0;

                let uid = token_data.local_id.clone();
                let mut document_data = HashMap::new();
                document_data.insert(
                    "score".to_string(),
                    Value {
                        value_type: Some(ValueType::IntegerValue(score.0)),
                    },
                );

                let document_path = format!("click/{uid}");

                ew.send(UpdateDocumentEvent {
                    document_path,
                    document_data,
                    id: 0,
                });
            }
        }
    }
}

fn logout_button_system(
    mut q_interaction: Query<(&Interaction,), (Changed<Interaction>, With<LogoutButton>)>,
    mut next_state: ResMut<NextState<AuthControllerState>>,
) {
    for (interaction,) in &mut q_interaction {
        if *interaction == Interaction::Clicked {
            // Go to in game state
            next_state.set(AuthControllerState::LogOut)
        }
    }
}

fn delete_account_button_system(
    mut q_interaction: Query<(&Interaction,), (Changed<Interaction>, With<DeleteAccountButton>)>,
    runtime: ResMut<TokioTasksRuntime>,
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    token_data: Option<Res<TokenData>>,
) {
    // TODO right shift fix
    if let Some(token_data) = token_data {
        for (interaction,) in &mut q_interaction {
            if *interaction == Interaction::Clicked {
                let mut client = client.0.clone();
                let project_id = project_id.0.clone();
                let uid = token_data.local_id.clone();

                runtime.spawn_background_task(|mut ctx| async move {
                    let _ =
                        async_delete_document(&mut client, &project_id, &format!("click/{}", uid))
                            .await;

                    ctx.run_on_main_thread(|ctx| {
                        ctx.world
                            .insert_resource(NextState(Some(AuthControllerState::Delete)));
                    })
                    .await;
                });
            }
        }
    }
}

// IN GAME

fn build_in_game(
    mut commands: Commands,
    mut q_ui_base: Query<Entity, With<UiBase>>,
    ui: Res<UiSettings>,
    score: Res<Score>,
) {
    println!("build_in_game");
    let ui_base = q_ui_base.single_mut();

    // UI
    commands.entity(ui_base).with_children(|parent| {
        // TITLE
        parent.spawn((
            TextBundle::from_section("in game", ui.typefaces.h2.clone()),
            InGameData,
        ));

        // SCORE
        parent.spawn((
            TextBundle::from_section(format!("score: {}", score.0), ui.typefaces.p.clone()),
            ScoreText,
            InGameData,
        ));

        // ADD SCORE BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(ScoreButton)
            .insert(InGameData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section(
                    "add score",
                    ui.typefaces.p.clone(),
                ));
            });

        // SUBMIT SCORE BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(SubmitScoreButton)
            .insert(InGameData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section(
                    "submit score",
                    ui.typefaces.p.clone(),
                ));
            });

        // RETURN TO MENU BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(ReturnToMenuButton)
            .insert(InGameData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section(
                    "back to menu",
                    ui.typefaces.p.clone(),
                ));
            });
    });
}

fn score_button_system(
    mut q_interaction: Query<(&Interaction,), (Changed<Interaction>, With<ScoreButton>)>,
    mut score: ResMut<Score>,
) {
    for (interaction,) in &mut q_interaction {
        if *interaction == Interaction::Clicked {
            score.0 += 1;
        }
    }
}

fn update_score(score: Res<Score>, mut q_score_text: Query<&mut Text, With<ScoreText>>) {
    if score.is_changed() {
        let mut score_text = q_score_text.single_mut();
        score_text.sections[0].value = format!("score: {}", score.0);
    }
}

fn submit_score_button_system(
    mut q_interaction: Query<(&Interaction,), (Changed<Interaction>, With<SubmitScoreButton>)>,
    score: Res<Score>,
    token_data: Res<TokenData>,
    runtime: ResMut<TokioTasksRuntime>,
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
) {
    for (interaction,) in &mut q_interaction {
        if *interaction == Interaction::Clicked {
            let uid = token_data.local_id.clone();
            let mut document_data = HashMap::new();
            document_data.insert(
                "score".to_string(),
                Value {
                    value_type: Some(ValueType::IntegerValue(score.0)),
                },
            );

            let document_path = format!("click/{uid}");
            let mut client = client.0.clone();
            let project_id = project_id.0.clone();

            runtime.spawn_background_task(|mut ctx| async move {
                //
                let _ =
                    async_update_document(&mut client, &project_id, &document_path, document_data)
                        .await;

                ctx.run_on_main_thread(|ctx| {
                    ctx.world
                        .insert_resource(NextState(Some(AppScreenState::Leaderboard)));
                })
                .await;
            });
        }
    }
}

fn return_to_menu_button_system(
    mut q_interaction: Query<(&Interaction,), (Changed<Interaction>, With<ReturnToMenuButton>)>,
    mut next_state: ResMut<NextState<AppScreenState>>,
) {
    for (interaction,) in &mut q_interaction {
        if *interaction == Interaction::Clicked {
            next_state.set(AppScreenState::MainMenu)
        }
    }
}

fn build_leaderboard(
    mut commands: Commands,
    mut q_ui_base: Query<Entity, With<UiBase>>,
    ui: Res<UiSettings>,
    mut ew: EventWriter<RunQueryEvent>,
) {
    println!("build_leaderboard");
    let ui_base = q_ui_base.single_mut();

    // Run query
    ew.send(RunQueryEvent {
        parent: "".into(),
        collection_id: "click".into(),
        limit: Some(10),
        order_by: ("score".into(), QueryDirection::Descending),
        id: 420,
    });

    commands.entity(ui_base).with_children(|parent| {
        // TITLE
        parent.spawn((
            TextBundle::from_section("leaderboard", ui.typefaces.h2.clone()).with_style(Style {
                ..Default::default()
            }),
            LeaderboardData,
        ));

        parent
            .spawn(NodeBundle {
                style: Style {
                    flex_direction: FlexDirection::Column,
                    size: Size {
                        width: Val::Px(300.),
                        height: Val::Px(400.),
                    },
                    ..default()
                },
                ..default()
            })
            .insert(Leaderboard)
            .insert(LeaderboardData);

        // RETURN TO MENU BUTTON
        parent
            .spawn(ui.button.clone())
            .insert(ReturnToMenuButton)
            .insert(LeaderboardData)
            .with_children(|parent| {
                parent.spawn(TextBundle::from_section(
                    "back to menu",
                    ui.typefaces.p.clone(),
                ));
            });
    });
}

struct UpdateLeaderboardEvent {
    responses: Result<Vec<RunQueryResponse>, Status>, // TODO simplify type
}

fn query_response_event_handler(
    mut er: EventReader<QueryResponseEvent>,
    mut ew: EventWriter<UpdateLeaderboardEvent>,
) {
    for e in er.iter() {
        match e.id {
            // This could be represented as an Enum #[repr(usize)]
            420 => {
                // This is our leaderboard event!
                ew.send(UpdateLeaderboardEvent {
                    responses: e.query_response.clone(),
                })
                // TODO emit UpdateLeaderboard event (with parsed responses?)
            }
            0 => {}
            _ => {}
        }

        // println!("QUERY RECEIVED: {:?}", e.query_response);
    }
}

fn update_leaderboard(
    mut er: EventReader<UpdateLeaderboardEvent>,
    mut q_leaderboard: Query<Entity, With<Leaderboard>>,
    mut commands: Commands,
    ui: Res<UiSettings>,
) {
    let leaderboard = q_leaderboard.single_mut();

    for e in er.iter() {
        match e.responses.clone() {
            Ok(responses) => {
                for response in responses {
                    // extract relevant data from response
                    let mut score: i64 = 0;
                    let mut nickname = "anon".into();

                    // TODO fix if-let hell
                    // fn main() {
                    //     fn f(_: bool, _: bool, _: bool) {}

                    //     let a = Some(true);
                    //     let b = Some(true);
                    //     let c = Some(true);

                    //     if let (Some(a), Some(b), Some(c)) = (a, b, c) {
                    //         f(a, b, c)
                    //     }
                    // }

                    if let Some(doc) = response.document {
                        nickname = if let Some(val) = doc.fields.get("nickname") {
                            if let Some(ValueType::StringValue(nickname)) = val.clone().value_type {
                                nickname
                            } else {
                                "anon".into()
                            }
                        } else {
                            "anon".into()
                        };

                        score = if let Some(val) = doc.fields.get("score") {
                            if let Some(ValueType::IntegerValue(score)) = val.clone().value_type {
                                score
                            } else {
                                0
                            }
                        } else {
                            0
                        };
                    };

                    commands.entity(leaderboard).with_children(|parent| {
                        // TODO align score/text
                        // column titles a la
                        //  score          name
                        //   696      xX_g4m3r_g0d_Xx
                        //   123          n00bz3r

                        parent.spawn(TextBundle::from_section(
                            format!("{}: {}", nickname, score),
                            ui.typefaces.p.clone(),
                        ));
                    });
                }
            }
            Err(err) => {
                // TODO write error message to leaderboard
                println!("LEADERBOARD ERROR:{:?}", err)
            }
        }
    }
}
