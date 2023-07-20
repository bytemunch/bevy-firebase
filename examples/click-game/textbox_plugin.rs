//! Quick and very very dirty text input plugin. I don't like any of it.

// TODO
// Text align selection
// Max chars
// Selectable text size
// Macro
// Find a fully fledged alternative this is a headache :)
// Just use egui

use bevy::{input::keyboard::KeyboardInput, prelude::*, text::TextLayoutInfo};

use crate::MainMenuData;

pub struct TextBoxPlugin;

impl Plugin for TextBoxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                listen_received_character_events,
                listen_keyboard_input_events,
                unfocus.before(handle_click_to_focus),
                handle_click_to_focus,
                highlight_focused,
            ),
        );
    }
}

pub fn create_text_box(cb: &mut ChildBuilder, font: Handle<Font>) {
    cb.spawn((
        NodeBundle {
            style: Style {
                width: Val::Px(300.),
                height: Val::Px(48.),
                overflow: Overflow::clip(),
                margin: UiRect {
                    top: Val::Px(5.),
                    ..default()
                },
                ..default()
            },
            background_color: Color::WHITE.into(),
            ..default()
        },
        Interaction::default(),
        TextInput,
    ))
    .with_children(|parent| {
        parent.spawn((TextBundle {
            text: Text::from_section(
                "".to_string(),
                TextStyle {
                    font,
                    font_size: 42.0,
                    color: Color::BLACK,
                },
            ),
            ..default()
        },));

        parent.spawn((
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    width: Val::Px(1.),
                    height: Val::Px(42.),
                    left: Val::Px(5.),
                    ..default()
                },
                background_color: Color::rgba(0., 0., 0., 1.).into(),
                visibility: Visibility::Hidden,
                ..default()
            },
            Cursor {
                timer: Timer::from_seconds(0.45, TimerMode::Repeating),
            },
        ));
    })
    .insert(MainMenuData); // TODO lifetimes and ting to return EntityCommands or something
}

#[derive(Component)]
struct Focused;

#[derive(Component)]
pub struct TextInput;

#[derive(Component)]
struct Cursor {
    timer: Timer,
}

fn unfocus(
    mut q_focused: Query<(Entity, &Children), With<Focused>>,
    mut commands: Commands,
    mouse: Res<Input<MouseButton>>,
    mut q_cursor: Query<&mut Visibility, With<Cursor>>,
) {
    if q_focused.is_empty() {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        let (focused, children) = q_focused.single_mut();
        let mut visibility = q_cursor.get_mut(children[1]).unwrap();
        commands.entity(focused).remove::<Focused>();
        *visibility = Visibility::Hidden;
    }
}

fn handle_click_to_focus(
    q_input: Query<(Entity, &Interaction), (Changed<Interaction>, With<TextInput>)>,
    mut windows: Query<&mut Window>,
    mut commands: Commands,
) {
    let mut window = windows.single_mut();

    for (entity, interaction) in &mut q_input.iter() {
        match interaction {
            Interaction::None => {
                window.cursor.icon = CursorIcon::Default;
                // TODO perftest/research this, does it assign repeatedly or is it a noop?
            }
            Interaction::Hovered => {
                window.cursor.icon = CursorIcon::Text;
            }
            Interaction::Pressed => {
                // FOCUS ELEMENT
                commands.entity(entity).insert(Focused);
            }
        }
    }
}

fn highlight_focused(
    mut q_cursor: Query<(&mut Visibility, &mut Style, &mut Cursor)>,
    q_text_info: Query<&TextLayoutInfo>,
    q_focused: Query<&Children, With<Focused>>,
    time: Res<Time>,
) {
    if q_focused.is_empty() {
        return;
    }

    let children = q_focused.single();

    let text_info = q_text_info.get(children[0]).unwrap();

    let (mut visibility, mut style, mut cursor) = q_cursor.get_mut(children[1]).unwrap();

    cursor.timer.tick(time.delta());

    if cursor.timer.just_finished() {
        if *visibility == Visibility::Hidden {
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }
    }

    let offset = text_info.size.x;

    // gotta love magic numbers that make things work with no explanation.
    style.left = Val::Px((offset * 0.93).max(5.));
}

fn listen_received_character_events(
    mut events: EventReader<ReceivedCharacter>,
    mut q_text: Query<&mut Text>,
    q_focused: Query<&Children, With<Focused>>,
) {
    if q_focused.is_empty() {
        return;
    }

    let children = q_focused.get_single().unwrap();

    let mut edit_text = q_text.get_mut(children[0]).unwrap();

    for event in events.iter() {
        // IF NOT RETURN
        if event.char != 0xD as char {
            edit_text.sections[0].value.push(event.char);
        }
    }
}

fn listen_keyboard_input_events(
    mut commands: Commands,
    mut events: EventReader<KeyboardInput>,
    mut q_text: Query<&mut Text>,
    q_focused: Query<(Entity, &Children), With<Focused>>,
    mut q_cursor: Query<&mut Visibility, With<Cursor>>,
) {
    if q_focused.is_empty() {
        return;
    }

    let (focused, children) = q_focused.get_single().unwrap();
    let mut edit_text = q_text.get_mut(children[0]).unwrap();
    let mut cursor_visibility = q_cursor.get_mut(children[1]).unwrap();

    for event in events.iter() {
        match event.key_code {
            Some(KeyCode::Return) => {
                commands.entity(focused).remove::<Focused>();
                *cursor_visibility = Visibility::Hidden;
            }
            Some(KeyCode::Back) => {
                edit_text.sections[0].value.pop();
            }
            _ => continue,
        }
    }
}
