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
    delete_document,
    deps::{Value, ValueType},
    update_document, BevyFirestoreClient, FirestoreState,
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
        // in game
        .add_system(build_in_game.in_schedule(OnEnter(AppScreenState::InGame)))
        .add_system(despawn_with::<InGameData>.in_schedule(OnExit(AppScreenState::InGame)))
        .add_system(update_score.in_set(OnUpdate(AppScreenState::InGame)))
        .add_system(score_button_system.in_set(OnUpdate(AppScreenState::InGame)))
        .add_system(return_to_menu_button_system.in_set(OnUpdate(AppScreenState::InGame)))
        .add_system(submit_score_button_system.in_set(OnUpdate(AppScreenState::InGame)))
        // leaderboard
        .add_system(build_leaderboard.in_schedule(OnEnter(AppScreenState::Leaderboard)))
        .add_system(
            despawn_with::<LeaderboardData>.in_schedule(OnExit(AppScreenState::Leaderboard)),
        )
        .add_system(return_to_menu_button_system.in_set(OnUpdate(AppScreenState::Leaderboard)))
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
struct LogoutButton;

#[derive(Component)]
struct PlayButton;

#[derive(Component)]
struct NicknameSubmitButton;

#[derive(Component)]
struct NicknameInput;

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

// GAME LOGIC
#[derive(Resource)]
struct Score(usize);

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

    // TODO load score from firestore
    commands.insert_resource(Score(0));
}

// UTILS

fn button_color_system(
    mut interaction_query: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, mut color) in &mut interaction_query {
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
    mut interaction_query: Query<(&Interaction,), (Changed<Interaction>, With<ExitButton>)>,
    mut exit: EventWriter<AppExit>,
) {
    for (interaction,) in &mut interaction_query {
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
    mut interaction_query: Query<
        (&Interaction, &LoginButton, &Children),
        (Changed<Interaction>, With<LoginButton>),
    >,
    mut text_query: Query<&mut Text>,
) {
    for (interaction, login_url, children) in &mut interaction_query {
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

fn firestore_ready(mut next_state: ResMut<NextState<AppScreenState>>) {
    println!("firestore ready!");
    next_state.set(AppScreenState::MainMenu);
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
        let name = "NAME HERE";
        // TODO grab name from firestore, if none found add placeholder

        parent.spawn((
            TextBundle::from_section(format!("welcome, {name}!"), ui.typefaces.p.clone()),
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
}

fn play_button_system(
    mut interaction_query: Query<(&Interaction,), (Changed<Interaction>, With<PlayButton>)>,
    mut next_state: ResMut<NextState<AppScreenState>>,
) {
    for (interaction,) in &mut interaction_query {
        if *interaction == Interaction::Clicked {
            // Go to in game state
            next_state.set(AppScreenState::InGame)
        }
    }
}

fn nickname_submit_button_system(
    mut interaction_query: Query<
        (&Interaction,),
        (Changed<Interaction>, With<NicknameSubmitButton>),
    >,
) {
    for (interaction,) in &mut interaction_query {
        if *interaction == Interaction::Clicked {
            // TODO
            println!("TODO: unimplemented function")
        }
    }
}

fn leaderboard_button_system(
    mut interaction_query: Query<(&Interaction,), (Changed<Interaction>, With<LeaderboardButton>)>,
    mut next_state: ResMut<NextState<AppScreenState>>,
) {
    for (interaction,) in &mut interaction_query {
        if *interaction == Interaction::Clicked {
            next_state.set(AppScreenState::Leaderboard)
        }
    }
}

fn delete_score_button_system(
    mut interaction_query: Query<(&Interaction,), (Changed<Interaction>, With<DeleteScoreButton>)>,
    mut score: ResMut<Score>,
    runtime: ResMut<TokioTasksRuntime>,
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    token_data: Option<Res<TokenData>>,
) {
    // TODO early return, tooooo much right shift
    if let Some(token_data) = token_data {
        for (interaction,) in &mut interaction_query {
            if *interaction == Interaction::Clicked {
                score.0 = 0;

                let mut client = client.clone();
                let project_id = project_id.0.clone();
                let uid = token_data.local_id.clone();
                let mut data = HashMap::new();
                data.insert(
                    "score".to_string(),
                    Value {
                        value_type: Some(ValueType::IntegerValue(score.0 as i64)),
                    },
                );

                runtime.spawn_background_task(|_ctx| async move {
                    // TODO errors
                    let _ = update_document(
                        &mut client,
                        &project_id,
                        &format!("click/{}", uid),
                        data.clone(),
                    )
                    .await;
                });
            }
        }
    }
}

fn logout_button_system(
    mut interaction_query: Query<(&Interaction,), (Changed<Interaction>, With<LogoutButton>)>,
    mut next_state: ResMut<NextState<AuthControllerState>>,
) {
    for (interaction,) in &mut interaction_query {
        if *interaction == Interaction::Clicked {
            // Go to in game state
            next_state.set(AuthControllerState::LogOut)
        }
    }
}

fn delete_account_button_system(
    mut interaction_query: Query<
        (&Interaction,),
        (Changed<Interaction>, With<DeleteAccountButton>),
    >,
    runtime: ResMut<TokioTasksRuntime>,
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    token_data: Option<Res<TokenData>>,
) {
    // TODO right shift fix
    if let Some(token_data) = token_data {
        for (interaction,) in &mut interaction_query {
            if *interaction == Interaction::Clicked {
                let mut client = client.clone();
                let project_id = project_id.0.clone();
                let uid = token_data.local_id.clone();

                runtime.spawn_background_task(|mut ctx| async move {
                    let _ =
                        delete_document(&mut client, &project_id, &format!("click/{}", uid)).await;

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
    mut interaction_query: Query<(&Interaction,), (Changed<Interaction>, With<ScoreButton>)>,
    mut score: ResMut<Score>,
) {
    for (interaction,) in &mut interaction_query {
        if *interaction == Interaction::Clicked {
            score.0 += 1;
        }
    }
}

fn update_score(score: Res<Score>, mut q_score_text: Query<&mut Text, With<ScoreText>>) {
    // TODO optimize, only set when score changed
    let mut score_text = q_score_text.single_mut();

    score_text.sections[0].value = format!("score: {}", score.0);
}

fn submit_score_button_system(
    mut interaction_query: Query<(&Interaction,), (Changed<Interaction>, With<SubmitScoreButton>)>,
    score: Res<Score>,
    mut next_state: ResMut<NextState<AppScreenState>>,
    runtime: ResMut<TokioTasksRuntime>,
    client: ResMut<BevyFirestoreClient>,
    project_id: Res<ProjectId>,
    token_data: Res<TokenData>,
) {
    for (interaction,) in &mut interaction_query {
        if *interaction == Interaction::Clicked {
            let mut client = client.clone();
            let project_id = project_id.0.clone();
            let uid = token_data.local_id.clone();
            let mut data = HashMap::new();
            data.insert(
                "score".to_string(),
                Value {
                    value_type: Some(ValueType::IntegerValue(score.0 as i64)),
                },
            );

            runtime.spawn_background_task(|_ctx| async move {
                // TODO errors
                let _ = update_document(
                    &mut client,
                    &project_id,
                    &format!("click/{}", uid),
                    data.clone(),
                )
                .await;
            });

            next_state.set(AppScreenState::Leaderboard);
        }
    }
}

fn return_to_menu_button_system(
    mut interaction_query: Query<(&Interaction,), (Changed<Interaction>, With<ReturnToMenuButton>)>,
    mut next_state: ResMut<NextState<AppScreenState>>,
) {
    for (interaction,) in &mut interaction_query {
        if *interaction == Interaction::Clicked {
            next_state.set(AppScreenState::MainMenu)
        }
    }
}

fn build_leaderboard(
    mut commands: Commands,
    mut q_ui_base: Query<Entity, With<UiBase>>,
    ui: Res<UiSettings>,
) {
    println!("build_leaderboard");
    let ui_base = q_ui_base.single_mut();

    commands.entity(ui_base).with_children(|parent| {
        // TITLE
        parent.spawn((
            TextBundle::from_section("leaderboard", ui.typefaces.h2.clone()).with_style(Style {
                ..Default::default()
            }),
            LeaderboardData,
        ));

        // TODO LEADERBOARD DISPLAY (scrollable UI w/ data from firestore)

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
