// CLICK
// click button, add to score
// has login
// has online leaderboard

mod util;

use bevy::prelude::*;
use bevy_firebase::{log_in, log_out, AuthState, GotAuthUrl};
use util::despawn_with;

// colours
const NORMAL_BUTTON: Color = Color::rgb(0.15, 0.15, 0.15);
const HOVERED_BUTTON: Color = Color::rgb(0.25, 0.25, 0.25);
const PRESSED_BUTTON: Color = Color::rgb(0.35, 0.75, 0.35);
const TEXT_COLOR: Color = Color::rgb(0.9, 0.9, 0.9);

#[derive(Default, States, Debug, Clone, Eq, PartialEq, Hash)]
enum AppAuthState {
    #[default]
    Start,
    LogIn,
    LogOut,
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
        // plugins
        .add_plugins(DefaultPlugins)
        .add_plugin(bevy_firebase::AuthPlugin {
            firebase_project_id: "test-auth-rs".into(),
            ..Default::default()
        })
        .add_plugin(bevy_firebase::FirestorePlugin {
            emulator_url: Some("http://127.0.0.1:8080".into()),
        })
        .add_plugin(bevy_tokio_tasks::TokioTasksPlugin::default())
        // states
        .add_state::<AppAuthState>()
        .add_state::<AppScreenState>()
        // init
        .add_startup_system(setup)
        // app-wide
        .add_system(button_color_system)
        // login
        .add_system(log_in.in_schedule(OnEnter(AppAuthState::LogIn)))
        .add_system(log_out.in_schedule(OnEnter(AppAuthState::LogOut)))
        // screens
        // login
        .add_system(build_login_screen.in_schedule(OnEnter(AppScreenState::LogInScreen)))
        .add_system(
            despawn_with::<LogInScreenData>.in_schedule(OnExit(AppScreenState::LogInScreen)),
        )
        .add_system(login_button_system.in_set(OnUpdate(AppScreenState::LogInScreen)))
        .add_system(logged_in.in_schedule(OnEnter(AuthState::LoggedIn)))
        .add_system(auth_url_listener.in_set(OnUpdate(AppAuthState::LogIn)))
        // menu
        .add_system(build_main_menu.in_schedule(OnEnter(AppScreenState::MainMenu)))
        .add_system(despawn_with::<MainMenuData>.in_schedule(OnExit(AppScreenState::MainMenu)))
        // in game
        .add_system(build_in_game.in_schedule(OnEnter(AppScreenState::InGame)))
        .add_system(despawn_with::<InGameData>.in_schedule(OnExit(AppScreenState::InGame)))
        // leaderboard
        .add_system(build_leaderboard.in_schedule(OnEnter(AppScreenState::Leaderboard)))
        .add_system(
            despawn_with::<LeaderboardData>.in_schedule(OnExit(AppScreenState::Leaderboard)),
        )
        .run();
}

#[derive(Resource, Clone)]
struct TitleTypeface {
    text_style: TextStyle,
    text_alignment: TextAlignment,
}

#[derive(Resource, Clone)]
struct ButtonTypeface {
    text_style: TextStyle,
    text_alignment: TextAlignment,
}

#[derive(Component)]
struct UiBase;

#[derive(Component)]
struct LoginButton(String);

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    let font = asset_server.load("fonts/HackNerdFont-Regular.ttf");

    commands.insert_resource(TitleTypeface {
        text_style: TextStyle {
            font: font.clone(),
            font_size: 60.0,
            color: TEXT_COLOR,
        },
        text_alignment: TextAlignment::Center,
    });

    commands.insert_resource(ButtonTypeface {
        text_style: TextStyle {
            font,
            font_size: 30.0,
            color: TEXT_COLOR,
        },
        text_alignment: TextAlignment::Center,
    });

    commands.spawn(Camera2dBundle::default());

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
        .insert(UiBase);
}

fn build_login_screen(
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppAuthState>>,
    mut q_ui_base: Query<Entity, With<UiBase>>,
    typeface: Res<TitleTypeface>,
) {
    println!("build_login_screen");
    let ui_base = q_ui_base.single_mut();

    // title
    commands.entity(ui_base).with_children(|parent| {
        parent.spawn((
            TextBundle::from_section("login", typeface.text_style.clone())
                .with_style(Style {
                    ..Default::default()
                })
                .with_text_alignment(typeface.text_alignment),
            LogInScreenData,
        ));
    });

    // attempt auto login
    next_state.set(AppAuthState::LogIn);
}

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
        }
    }
}

fn auth_url_listener(
    mut commands: Commands,
    mut er: EventReader<GotAuthUrl>,
    mut q_ui_base: Query<Entity, With<UiBase>>,
    typeface: Res<ButtonTypeface>,
) {
    for e in er.iter() {
        println!("Go to this URL to sign in:\n{}\n", e.0);

        // add login button
        let ui_base = q_ui_base.single_mut();

        commands.entity(ui_base).with_children(|parent| {
            parent
                .spawn(ButtonBundle {
                    style: Style {
                        size: Size::new(Val::Px(300.0), Val::Px(65.0)),
                        // horizontally center child text
                        justify_content: JustifyContent::Center,
                        // vertically center child text
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    background_color: NORMAL_BUTTON.into(),
                    ..Default::default()
                })
                .insert(LoginButton(e.0.clone().into()))
                .insert(LogInScreenData)
                .with_children(|parent| {
                    parent.spawn(
                        TextBundle::from_section("log in with google", typeface.text_style.clone())
                            .with_style(Style::default())
                            .with_text_alignment(typeface.text_alignment),
                    );
                });
        });
    }
}

fn logged_in(mut next_state: ResMut<NextState<AppScreenState>>) {
    println!("logged_in");
    // set app state to main menu
    next_state.set(AppScreenState::MainMenu);
}

fn build_main_menu(
    mut commands: Commands,
    mut q_ui_base: Query<Entity, With<UiBase>>,
    typeface: Res<TitleTypeface>,
) {
    println!("build_main_menu");
    let ui_base = q_ui_base.single_mut();

    // title
    commands.entity(ui_base).with_children(|parent| {
        parent.spawn((
            TextBundle::from_section("main menu", typeface.text_style.clone())
                .with_style(Style {
                    ..Default::default()
                })
                .with_text_alignment(typeface.text_alignment),
            LogInScreenData,
        ));
    });
    // play button

    // nickname text entry
    // nickname submit button

    // log out button

    // delete account button
}

fn build_in_game(mut _commands: Commands) {
    println!("build_in_game");
    // title
    // score
    // add score button
    // submit score button
    // exit to menu button
}

fn build_leaderboard(mut _commands: Commands) {
    println!("build_leaderboard");
    // title
    // auto-updating leaderboard connected to firestore

    // exit to menu button
}
