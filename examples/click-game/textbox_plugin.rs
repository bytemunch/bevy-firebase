use bevy::{
    input::keyboard::KeyboardInput,
    prelude::*,
    text::{BreakLineOn, TextLayoutInfo},
    ui::{widget::TextFlags, ContentSize, FocusPolicy},
};

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
        )
        .add_systems(Startup, setup_cursor);
    }
}

#[derive(Component, Debug)]
pub struct TextBox;

/// A UI node that is text
#[derive(Bundle, Debug)]
pub struct TextBoxBundle {
    // TEXTBOX BITS
    /// Describes the logical size of the node
    pub node: Node,
    /// Styles which control the layout (size and position) of the node and it's children
    /// In some cases these styles also affect how the node drawn/painted.
    pub style: Style,
    /// Contains the text of the node
    pub text: Text,
    /// Text layout information
    pub text_layout_info: TextLayoutInfo,
    /// Text system flags
    pub text_flags: TextFlags,
    /// The calculated size based on the given image
    pub calculated_size: ContentSize,
    /// Whether this node should block interaction with lower nodes
    pub focus_policy: FocusPolicy,
    /// The transform of the node
    ///
    /// This field is automatically managed by the UI layout system.
    /// To alter the position of the `NodeBundle`, use the properties of the [`Style`] component.
    pub transform: Transform,
    /// The global transform of the node
    ///
    /// This field is automatically managed by the UI layout system.
    /// To alter the position of the `NodeBundle`, use the properties of the [`Style`] component.
    pub global_transform: GlobalTransform,
    /// Describes the visibility properties of the node
    pub visibility: Visibility,
    /// Algorithmically-computed indication of whether an entity is visible and should be extracted for rendering
    pub computed_visibility: ComputedVisibility,
    /// Indicates the depth at which the node should appear in the UI
    pub z_index: ZIndex,
    /// The background color that will fill the containing node
    pub background_color: BackgroundColor,
    // EXTRA BITS
    pub interaction: Interaction,
    pub tag: TextBox,
}

impl Default for TextBoxBundle {
    fn default() -> Self {
        Self {
            text: Default::default(),
            text_layout_info: Default::default(),
            text_flags: Default::default(),
            calculated_size: Default::default(),
            background_color: BackgroundColor(Color::WHITE),
            node: Default::default(),
            style: Style {
                overflow: Overflow::clip(),
                ..default()
            },
            focus_policy: Default::default(),
            transform: Default::default(),
            global_transform: Default::default(),
            visibility: Default::default(),
            computed_visibility: Default::default(),
            z_index: Default::default(),
            interaction: Default::default(),
            tag: TextBox,
        }
    }
}

#[allow(dead_code)]
impl TextBoxBundle {
    /// Create a [`TextBundle`] from a single section.
    ///
    /// See [`Text::from_section`] for usage.
    pub fn from_section(value: impl Into<String>, style: TextStyle) -> Self {
        Self {
            text: Text::from_section(value, style),
            ..Default::default()
        }
    }

    /// Create a [`TextBundle`] from a list of sections.
    ///
    /// See [`Text::from_sections`] for usage.
    pub fn from_sections(sections: impl IntoIterator<Item = TextSection>) -> Self {
        Self {
            text: Text::from_sections(sections),
            ..Default::default()
        }
    }

    /// Returns this [`TextBundle`] with a new [`TextAlignment`] on [`Text`].
    pub const fn with_text_alignment(mut self, alignment: TextAlignment) -> Self {
        self.text.alignment = alignment;
        self
    }

    /// Returns this [`TextBundle`] with a new [`Style`].
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Returns this [`TextBundle`] with a new [`BackgroundColor`].
    pub const fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = BackgroundColor(color);
        self
    }

    /// Returns this [`TextBundle`] with soft wrapping disabled.
    /// Hard wrapping, where text contains an explicit linebreak such as the escape sequence `\n`, will still occur.
    pub const fn with_no_wrap(mut self) -> Self {
        self.text.linebreak_behavior = BreakLineOn::NoWrap;
        self
    }
}

// Systems

#[derive(Component)]
struct Focused;

#[derive(Component)]
struct Cursor {
    timer: Timer,
}

fn setup_cursor(mut commands: Commands) {
    commands
        .spawn(NodeBundle {
            style: Style {
                position_type: PositionType::Absolute,
                width: Val::Px(1.),
                height: Val::Px(30.),
                left: Val::Px(5.),
                ..default()
            },
            background_color: Color::rgba(0., 0., 0., 1.).into(),
            visibility: Visibility::Hidden,
            z_index: ZIndex::Global(10),
            ..default()
        })
        .insert(Cursor {
            timer: Timer::from_seconds(0.45, TimerMode::Repeating),
        });
}

fn unfocus(
    q_focused: Query<Entity, With<Focused>>,
    mut commands: Commands,
    mouse: Res<Input<MouseButton>>,
    mut q_cursor: Query<&mut Visibility, With<Cursor>>,
) {
    if q_focused.is_empty() {
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        let focused = q_focused.single();
        commands.entity(focused).remove::<Focused>();
        let mut visibility = q_cursor.single_mut();
        *visibility = Visibility::Hidden;
    }
}

fn handle_click_to_focus(
    q_input: Query<(Entity, &Interaction), (Changed<Interaction>, With<TextBox>)>,
    mut windows: Query<&mut Window>,
    mut commands: Commands,
) {
    let mut window = windows.single_mut();

    for (entity, interaction) in &mut q_input.iter() {
        match interaction {
            Interaction::None => {
                // TODO bad way of doing this, should set to default at start of each frame and modify if needed
                window.cursor.icon = CursorIcon::Default;
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
    q_text_info: Query<(&Node, &GlobalTransform, &TextLayoutInfo), With<Focused>>,
    time: Res<Time>,
) {
    if q_text_info.is_empty() {
        return;
    }

    if q_cursor.is_empty() {
        return;
    }

    let (node, transform, text_info) = q_text_info.single();

    let (mut visibility, mut style, mut cursor) = q_cursor.single_mut();

    cursor.timer.tick(time.delta());

    if cursor.timer.just_finished() {
        if *visibility == Visibility::Hidden {
            *visibility = Visibility::Visible;
        } else {
            *visibility = Visibility::Hidden;
        }
    }

    // Doesn't work multiline.
    let offset_x = text_info.size.x;

    let text_box_left = transform.translation().x - node.logical_rect(transform).width() / 2.;
    let text_box_top = 3. + transform.translation().y - node.logical_rect(transform).height() / 2.;

    // gotta love magic numbers that make things work with no explanation.
    style.left = Val::Px((offset_x * 0.93).max(5.) + text_box_left);
    style.top = Val::Px(text_box_top);
}

fn listen_received_character_events(
    mut events: EventReader<ReceivedCharacter>,
    mut q_focused: Query<&mut Text, With<Focused>>,
) {
    if q_focused.is_empty() {
        return;
    }

    let mut edit_text = q_focused.single_mut();

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
    mut q_focused: Query<(Entity, &mut Text), With<Focused>>,
    mut q_cursor: Query<&mut Visibility, With<Cursor>>,
) {
    if q_focused.is_empty() {
        return;
    }

    let (focused, mut edit_text) = q_focused.single_mut();
    let mut cursor_visibility = q_cursor.single_mut();

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
