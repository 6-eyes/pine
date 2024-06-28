use ::core::fmt;
use core::StoreError;
use std::{sync::Arc, thread::sleep, time::Duration};
use iced::{alignment, clipboard, executor, font::Weight, widget::{button, column, container, horizontal_space, keyed_column, radio, row, text, text_editor, text_input, Column, Container, Row}, window::{self, Position}, Alignment, Application, Command, Element, Font, Length, Pixels, Settings, Size};

const TITLE: &str = "pine";

fn main() -> iced::Result {
    let settings = Settings {
        window: window::Settings {
            size: Size::new(800f32, 800f32),
            resizable: true,
            decorations: true,
            position: Position::Default,
            visible: true,
            transparent: false,
            ..Default::default()
        },
        fonts: vec!{ include_bytes!("../fonts/pine-icons.ttf").as_slice().into() },
        ..Default::default()
    };
    Pine::run(settings)
}

struct Pine {
    cred_list: Vec<Cred>,
    insert_mode: InsertMode,
    toasts: Vec<Toast>,
    storage: Arc<core::Storage>,
}

impl Application for Pine {
    type Executor = executor::Default;
    type Flags = ();
    type Message = Message;
    type Theme = theme::Theme;

    fn new(_flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let pine = Pine {
            cred_list: Vec::new(),
            insert_mode: InsertMode::Disabled,
            toasts: vec!{ Toast { message: "Add new credential".to_string(), status: Status::Info } },
            storage: Arc::new(core::Storage::new_from_secret("    ")),
        };
        let fetched_fn = |res: Result<Vec<(String, Secret, String)>, StoreError>| {
            match res {
                Ok(cred_list) => Message::Storage(core::StoreMessage::Fetched(cred_list)),
                Err(e) => Message::Invalid(e.into()),
            }
        };
        let command = Command::perform(core::fetch(Arc::clone(&pine.storage)), fetched_fn);
        (pine, command)
    }

    fn title(&self) -> String {
        TITLE.to_string()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::InsertToggle => self.insert_mode = match self.insert_mode {
                    InsertMode::Enabled(_) => InsertMode::Disabled,
                    InsertMode::Disabled => InsertMode::Enabled(CredInsert::default()),
            },
            Message::UsernameInput(username) => if let InsertMode::Enabled(fields) = &mut self.insert_mode {
                    fields.username = username;
            },
            Message::SecretInput(secret) => if let InsertMode::Enabled(fields) = &mut self.insert_mode { fields.secret.set_secret(secret) },
            Message::Add => if let InsertMode::Enabled(fields) = &mut self.insert_mode {
                match Cred::new(std::mem::take(&mut fields.username), std::mem::take(&mut fields.secret), std::mem::take(&mut fields.description.text())) {
                    Ok(new_cred) => self.cred_list.push(new_cred),
                    Err(_) => { eprintln!("no secret passed") },
                };
                fields.username = String::default();
                fields.secret = SecretInput::default();
                fields.description = text_editor::Content::default();
                self.insert_mode = InsertMode::Disabled;
                return self.update_repo(None);
            },
            Message::DescriptionInput(action) => if let InsertMode::Enabled(fields) = &mut self.insert_mode {
                fields.description.perform(action);
            },
            Message::Cancel => self.insert_mode = InsertMode::Disabled,
            Message::Action(i, action) => return self.update_cred(i, action),
            Message::ToggleSecretReveal => if let InsertMode::Enabled(fields) = &mut self.insert_mode {
                fields.reveal_secret = !fields.reveal_secret;
            },
            Message::GenerateRandom => {}
            Message::SecretType(secret_type) => if let InsertMode::Enabled(fields) = &mut self.insert_mode {
                fields.secret = match secret_type {
                    SecretTypeMessage::Pin => SecretInput::Pin({
                        match &fields.secret {
                            SecretInput::Pin(val) if val.is_some() => Some(val.unwrap()),
                            SecretInput::Password(val) => {
                                match val.parse() {
                                    Ok(i) => Some(i),
                                    Err(_) => None,
                                }
                            },
                            _ => None,
                        }
                    }),
                    SecretTypeMessage::Password => SecretInput::Password({
                        match &fields.secret {
                            SecretInput::Password(val) => val.to_owned(),
                            SecretInput::Pin(val) if val.is_some() => val.unwrap().to_string(),
                            _ => String::default(),
                        }
                    }),
                };
            },
            Message::CloseToast(i) => {
                if self.toasts.get(i).is_some() {
                    self.toasts.remove(i);
                }
            },
            Message::Storage(store_message) => match store_message {
                core::StoreMessage::Added => self.toast("New credential added", Status::Success),
                core::StoreMessage::Updated => self.toast("Credential updated", Status::Success),
                core::StoreMessage::Deleted => self.toast("Credential deleted", Status::Success),
                core::StoreMessage::Fetched(cred_list) => {
                    self.cred_list.extend(cred_list.into_iter().map(|cred| Cred::new_from_raw(cred.0, cred.1, cred.2)));
                },
                core::StoreMessage::Invalid => self.toast("Some error occurred", Status::Danger)
            },
            Message::Invalid(e) => self.toast(e.as_str(), Status::Danger),
        };
        Command::none()
    }

    fn view(&self) -> Element<Self::Message, Self::Theme> {
        let mut col = column!{ text(TITLE).style(theme::Text::Title).size(100) };
        col = match &self.insert_mode {
            InsertMode::Disabled => {
                let button = button(button_content(Some('\u{E803}'), Some("New"), Length::Fill, None)).padding([20, 20, 20, 20]).on_press(Message::InsertToggle);
                col.push(row!{ horizontal_space(), button, horizontal_space() })
            },
            InsertMode::Enabled(message) => {
                let type_selector = {
                    let mut row = Row::new();
                    for typ in [SecretTypeMessage::Password, SecretTypeMessage::Pin] {
                        let (label, selected) = match typ {
                            SecretTypeMessage::Password => ("Password", if let SecretInput::Password(_) = message.secret { Some(typ) } else { None }),
                            SecretTypeMessage::Pin => ("Pin", if let SecretInput::Pin(_) = message.secret { Some(typ) } else { None }),
                        };
                        row = row.push( radio(label, typ, selected, Message::SecretType) );
                    }
                    row.spacing(40)
                };
                let cred_fields = {
                    let (secret_type, value) = match &message.secret {
                        SecretInput::Password(val) => ("password", val.to_owned()),
                        SecretInput::Pin(val) => ("pin", val.as_ref().map(u32::to_string).unwrap_or(String::default())),
                    };
                    let secret_row = row!{ text_input(secret_type, &value).secure(!message.reveal_secret).on_input(Message::SecretInput), button(button_content(Some(if message.reveal_secret {'\u{E801}'} else {'\u{E802}'}), None, Length::Shrink, None)).on_press(Message::ToggleSecretReveal), button(button_content(Some('\u{E800}'), None, Length::Shrink, Some(theme::Text::Black))).style(theme::Button::Distinct).on_press(Message::GenerateRandom) }.spacing(5);
                    row!{ text_input("username", &message.username).on_input(Message::UsernameInput), secret_row }.spacing(20)
                };
                let disc = text_editor(&message.description).on_action(Message::DescriptionInput);
                let action_buttons = row!{ button(button_content(None, Some("Cancel"), Length::Fill, None)).on_press(Message::Cancel), button(button_content(None, Some("Add"), Length::Fill, None)).on_press_maybe( message.is_not_empty().then(|| Message::Add))}.spacing(20);
                col.push(type_selector).push(cred_fields).push(disc).push(action_buttons)
            }
        };
        let list = keyed_column(self.cred_list.iter().enumerate().map(|(i, cred)| (i, cred.view().map(move |ca| Message::Action(i as i32, ca))))).spacing(20);
        let content = container(col.push(list).align_items(alignment::Alignment::Center).spacing(20).max_width(Pixels::from(800))).padding([0, 20, 0, 20]).center_x();
        display_manager::Manager::new(content, &self.toasts, Message::CloseToast).into()
    }

    fn theme(&self) -> Self::Theme {
        Default::default()
    }
}

impl Pine {
    async fn secret_reveal_timeout(sec: u64) {
        sleep(Duration::from_secs(sec));
    }

    fn update_cred(&mut self, i: i32, action: CredAction) -> Command<Message> {
        if let Some(cred) = self.cred_list.get_mut(i as usize) {
            match action {
                CredAction::Reveal => {
                    cred.hidden = false;
                    return Command::perform(Self::secret_reveal_timeout(5), move |_| Message::Action(i, CredAction::Hide));
                },
                CredAction::Save => {
                    cred.set_creds();
                },
                CredAction::ToggleEdit => cred.toggle_edit(),
                CredAction::Hide => cred.hidden = true,
                CredAction::YankUsername => return clipboard::write(cred.username.0.to_owned()),
                CredAction::YankSecret => return clipboard::write(cred.secret.value(false)),
                CredAction::Delete => {
                    self.cred_list.remove(i as usize);
                    return self.update_repo(Some(action));
                },
                CredAction::DescriptionInput(_) | CredAction::SecretInput(_) | CredAction::UsernameInput(_) => cred.update(action),
            }
        }
        Command::none()
    }

    fn toast(&mut self, message: &str, status: Status) {
        self.toasts.push(Toast { message: message.to_owned(), status });
    }

    fn update_repo(&self, action: Option<CredAction>) -> Command<Message> {
        let store_result = |res: Result<(), core::StoreError>| match res {
            Ok(_) => {
                let message = match action {
                    None => core::StoreMessage::Added,
                    Some(action) => {
                        match action {
                            CredAction::Delete => core::StoreMessage::Deleted,
                            CredAction::Save => core::StoreMessage::Updated,
                            _ => core::StoreMessage::Invalid,
                        }
                    }
                };
                Message::Storage(message)
            },
            Err(e) => Message::Invalid(e.into()),
        };
        let creds_cloned = self.cred_list.iter().map(|cred| (cred.username.0.clone(), cred.secret.clone(), cred.description.0.clone())).collect::<Vec::<(String, Secret, String)>>();
        Command::perform(core::save(Arc::clone(&self.storage), creds_cloned), store_result)
    }
}

fn button_content<'a, Message: Clone + 'a>(codepoint: Option<char>, string: Option<&str>, width: Length, text_style: Option<theme::Text>) -> Element<'a, Message, theme::Theme> {
    const ICON_FONT: Font = Font::with_name("pine-icons");
    let style = match text_style {
        Some(s) => s,
        None => theme::Text::Default,
    };
    let content = Row::new().push_maybe(codepoint.map(|cp| text(cp).style(style).font(ICON_FONT))).push_maybe(string.map(text)).spacing(10);
    container(content).width(width).height(Length::Shrink).center_x().into()
}

#[derive(Debug)]
struct Cred {
    username: Username,
    secret: Secret,
    description: Description,
    hidden: bool,
    edit_mode: Option<CredEdit>,
}

struct NoSecret;

impl Cred {
    fn new(username: String, secret: SecretInput, description: String) -> Result<Self, NoSecret> {
        let secret = match secret {
            SecretInput::Password(pass) => Secret::Password(pass),
            SecretInput::Pin(pin) if pin.is_some() => Secret::Pin(pin.unwrap().to_string()),
            _ => return Err(NoSecret),
        };
        Ok(Self {
            username: Username::new(username),
            secret,
            description: Description::new(description),
            hidden: true,
            edit_mode: None,
        })
    }

    fn new_from_raw(username: String, secret: Secret, description: String) -> Self {
        Self {
            username: Username::new(username),
            secret,
            description: Description::new(description),
            hidden: true,
            edit_mode: None,
        }
    }
    
    fn view(&self) -> Element<CredAction, theme::Theme> {
        let details_col = {
            let cred_row = row!(self.username.view(self.edit_mode.as_ref().map(|em| em.username.as_ref())), self.secret.view(self.hidden, self.edit_mode.as_ref().map(|em| &em.secret))).spacing(5);
            column!( cred_row, self.description.view(self.edit_mode.as_ref().map(|em| &em.description)) ).spacing(5).width(Length::Fill)
        };
        let action_col = {
            let button_from_icon = |cp: char, a: Option<CredAction>| button(button_content(Some(cp), None, Length::Fixed(20f32), None)).on_press_maybe(a);
            let save = self.edit_mode.as_ref().map(|ce| button_from_icon('\u{E808}', ce.is_not_empty().then(|| CredAction::Save)));
            Column::new().push_maybe(save).push(button_from_icon(if self.edit_mode.is_some() { '\u{E807}'} else { '\u{E804}' }, Some(CredAction::ToggleEdit))).push(button_from_icon('\u{E805}', Some(CredAction::Delete))).spacing(4)
        };
        container(row!( details_col, action_col ).spacing(4).padding(8).height(Length::Shrink)).style(theme::Container::Cred).into()
    }

    fn toggle_edit(&mut self) {
        self.edit_mode = match self.edit_mode {
            Some(_) => None,
            None => Some(CredEdit::new_from(&self.username.0, &self.secret.value(false), self.secret.kind(), &self.description.0)),
        };
    }

    fn update(&mut self, action: CredAction) {
        if let Some(ce) = &mut self.edit_mode {
            if let CredAction::UsernameInput(username) = action {
                ce.username = username;
            }
            else if let CredAction::SecretInput(secret) = action {
                ce.secret.set_secret(secret);
            }
            else if let CredAction::DescriptionInput(description) = action {
                ce.description.perform(description);
            }
        }
    }

    fn set_creds(&mut self) {
        if let Some(new_values) = &mut self.edit_mode {
            self.username.update(std::mem::take(&mut new_values.username));
            self.secret.update(std::mem::take(&mut new_values.secret));
            self.description.update(std::mem::take(&mut new_values.description));
            self.toggle_edit();
        }
    }
}

enum InsertMode {
    Enabled(CredInsert),
    Disabled,
}

#[derive(Clone, Debug)]
pub enum Message {
    InsertToggle,
    Action(i32, CredAction),
    UsernameInput(String),
    SecretInput(String),
    DescriptionInput(text_editor::Action),
    Add,
    Cancel,
    ToggleSecretReveal,
    GenerateRandom,
    SecretType(SecretTypeMessage),
    CloseToast(usize),
    Storage(core::StoreMessage),
    Invalid(String),
}

#[derive(Clone, Debug, Copy, Eq, PartialEq)]
pub enum SecretTypeMessage {
    Password,
    Pin,
}

impl fmt::Display for SecretTypeMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password => "Password",
            Self::Pin => "Pin",
        }.fmt(f)
    }
}

#[derive(Clone, Debug)]
pub enum CredAction {
    Delete,
    ToggleEdit,
    Save,
    UsernameInput(String),
    SecretInput(String),
    DescriptionInput(text_editor::Action),
    Reveal,
    Hide,
    YankUsername,
    YankSecret,
}

#[derive(Debug, Default)]
struct CredEdit {
    username: String,
    secret: SecretInput,
    description: text_editor::Content,
}

impl CredEdit {
    fn is_not_empty(&self) -> bool {
        !self.username.is_empty() && !self.secret.is_empty()
    }

    fn new_from(username: &str, secret: &str, kind: SecretTypeMessage, description: &str) -> Self {
        Self {
            username: username.to_owned(),
            secret: SecretInput::new_from(secret, kind),
            description: text_editor::Content::with_text(description),
        }
    }
}

#[derive(Default)]
struct CredInsert {
    username: String,
    secret: SecretInput,
    reveal_secret: bool,
    description: text_editor::Content,
}

impl CredInsert {
    fn is_not_empty(&self) -> bool {
        !self.username.is_empty() && !self.secret.is_empty()
    }
}

#[derive(Clone, Debug)]
enum SecretInput {
    Password(String),
    Pin(Option<u32>),
}

impl Default for SecretInput {
    fn default() -> Self {
        Self::Password(String::default())
    }
}

impl SecretInput {
    fn is_empty(&self) -> bool {
        match self {
            Self::Password(value) => value.is_empty(),
            Self::Pin(value) => value.is_none(),
        }
    }

    fn set_secret(&mut self, secret: String) {
        match self {
            Self::Password(pass) => *pass = secret,
            Self::Pin(pass) => *pass = match secret.is_empty() {
                true => None,
                false => match secret.parse() {
                    Ok(val) => Some(val),
                    Err(_) => *pass,
                }
            },
        }
    }

    fn new_from(val: &str, kind: SecretTypeMessage) -> Self {
        match kind {
            SecretTypeMessage::Password => Self::Password(val.to_owned()),
            SecretTypeMessage::Pin => Self::Pin(val.parse().ok()),
        }
    }

    fn get_val(&self) -> String {
        match self {
            Self::Password(val) => val.to_owned(),
            Self::Pin(val) => val.map(|v| v.to_string()).unwrap_or(String::default()),
        }
    }
}

#[derive(Clone, Debug)]
struct Username(String);

impl Username {
    fn new(username: String) -> Self {
        Self(username)
    }

    fn view(&self, edit_mode: Option<&str>) -> Container<CredAction, theme::Theme> {
        let content: Element<CredAction, theme::Theme> = match edit_mode {
            Some(input) => text_input("username", input).on_input(CredAction::UsernameInput).into(),
            None => {
                let title = text("Username:").style(theme::Text::Title).font(Font { weight: Weight::Bold, ..Default::default() });
                let text = text(&self.0).style(theme::Text::Light);
                let copy_button = button(button_content(Some('\u{E806}'), None, Length::Shrink, Some(theme::Text::Gray))).style(theme::Button::Cred).on_press(CredAction::YankUsername);
                row!(title, text, copy_button).spacing(8).align_items(Alignment::Center).into()
            },
        };
        container(content).center_x().width(Length::Fill)
    }

    fn update(&mut self, username: String) {
        self.0 = username;
    }
}

#[derive(Clone, Debug)]
pub enum Secret {
    Password(String),
    Pin(String),
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, secret) = match self {
            Self::Password(sec) => ("password", sec.as_str()),
            Self::Pin(sec) => ("pin", sec.as_str()),
        };
        write!(f, "{}:{}", kind, secret)
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self::from(value.as_str())
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        let (kind, secret) = value.split_once(':').unwrap();
        match kind {
            "password" => Self::Password(secret.to_owned()),
            "pin" => Self::Pin(secret.to_owned()),
            _ => unreachable!(),
        }
    }
}

impl Secret {
    fn value(&self, hidden: bool) -> String {
        let secret = match self {
            Self::Password(value) => value,
            Self::Pin(value) => value,
        };
        match hidden {
            true => format!("{:•^1$}", String::default(), secret.len()),
            false => secret.to_owned(),
        }
    }

    fn kind(&self) -> SecretTypeMessage {
        match self {
            Self::Password(_) => SecretTypeMessage::Password,
            Self::Pin(_) => SecretTypeMessage::Pin,
        }
    }

    fn view(&self, hidden: bool, edit_mode: Option<&SecretInput>) -> Container<CredAction, theme::Theme> {
        let content: Element<CredAction, theme::Theme> = match edit_mode {
            Some(val) => {
                let kind = match val {
                    SecretInput::Password(_) => "password",
                    SecretInput::Pin(_) => "pin",
                };
                text_input(kind, &val.get_val()).on_input(CredAction::SecretInput).into()
            },
            None => {
                let kind = match self {
                    Self::Password(_) => "Password",
                    Self::Pin(_) => "Pin",
                };
                let title = text( kind ).style(theme::Text::Title).font(Font { weight: Weight::Bold, ..Default::default() });
                let text = text(self.value(hidden)).style(theme::Text::Light);
                let view_button = button(button_content(Some(if hidden {'\u{E802}'} else {'\u{E801}'}), None, Length::Shrink, Some(theme::Text::Gray))).style(theme::Button::Cred).on_press(if hidden { CredAction::Reveal } else { CredAction::Hide });
                let copy_button = button(button_content(Some('\u{E806}'), None, Length::Shrink, Some(theme::Text::Gray))).style(theme::Button::Cred).on_press(CredAction::YankSecret);
                row!(title, text, view_button, copy_button).spacing(8).align_items(Alignment::Center).into()
            },
        };

        container(content).center_x().width(Length::Fill)
    }

    fn update(&mut self, input: SecretInput) {
        match self {
            Self::Password(pass) => *pass = match input {
                SecretInput::Password(val) => val,
                SecretInput::Pin(_) => String::default(),
            },
            Self::Pin(pin) => *pin = match input {
                SecretInput::Pin(val) if val.is_some() => val.unwrap().to_string(),
                _ => String::default(),
            }
        }
    }
}

#[derive(Debug)]
struct Description(String);

impl Description {
    fn new(description: String) -> Self {
        Self(description.trim().to_owned())
    }

    fn view<'a>(&'a self, edit_mode: Option<&'a text_editor::Content>) -> Element<CredAction, theme::Theme>  {
        match edit_mode {
            Some(description) => text_editor(description).on_action(CredAction::DescriptionInput).height(Length::Fill).into(),
            None => text(&self.0).style(theme::Text::Light).into(),
        }
    }

    fn update(&mut self, description: text_editor::Content) {
        self.0 = description.text();
    }
}

pub struct Toast {
    message: String,
    status: Status,
}

enum Status {
    Info,
    Success,
    Danger,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => "Info",
            Self::Success => "Success",
            Self::Danger => "Danger",
        }.fmt(f)
    }
}

mod display_manager {
    use std::time::{Duration, Instant};
    use iced::{advanced::{graphics::core::event, layout, overlay, widget::{self, Tree}, Layout, Shell, Widget}, widget::{button, container, row, text}, window, Alignment, Element, Event, Length, Point, Renderer, Size};
    use crate::{Message, theme::{Theme, Container, Text, Button}, Toast, button_content, Status};

    const TOAST_DURATION: Duration = Duration::from_secs(3);

    pub struct Manager<'a> {
        content: Element<'a, Message, Theme>,
        toasts: Vec<Element<'a, Message, Theme, Renderer>>,
        on_close: Box<dyn Fn(usize) -> Message + 'a>,
    }

    impl<'a> Manager<'a> {
        pub fn new(content: impl Into<Element<'a, Message, Theme>>, toasts: &'a [Toast], on_close: impl Fn(usize) -> Message + 'a) -> Self {
            let toasts = toasts.iter().enumerate().map(|(index, toast)| {
                let (text_style, container_style) = match toast.status {
                    Status::Info => (Text::Black, Container::InfoToast),
                    Status::Success => (Text::Black, Container::SuccessToast),
                    Status::Danger => (Text::Black, Container::DangerToast),
                };
                let row = row!( text(&toast.message), button(button_content(Some('\u{E807}'), None, Length::Shrink, Some(text_style))).style(Button::Toast).on_press(on_close(index)) ).spacing(5).width(Length::Shrink).align_items(Alignment::Center);
                container(row).padding(5).style(container_style).into()
            }).collect();
            Self {
                content: content.into(),
                toasts,
                on_close: Box::new(on_close),
            }
        }
    }

    impl<'a> Widget<Message, Theme, Renderer> for Manager<'a> {
        fn size(&self) -> Size<Length> {
            self.content.as_widget().size()
        }

        fn layout(&self, tree: &mut widget::Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
            self.content.as_widget().layout(&mut tree.children[0], renderer, limits)
        }

        fn draw(&self, state: &widget::Tree, renderer: &mut Renderer, theme: &Theme, style: &iced::advanced::renderer::Style, layout: Layout<'_>, cursor: iced::advanced::mouse::Cursor, viewport: &iced::Rectangle) {
            self.content.as_widget().draw(&state.children[0], renderer, theme, style, layout, cursor, viewport);
        }

        fn children(&self) -> Vec<Tree> {
            std::iter::once(Tree::new(&self.content)).chain(self.toasts.iter().map(Tree::new)).collect()
        }

        fn state(&self) -> widget::tree::State {
            widget::tree::State::new(Vec::<Option<Instant>>::new())
        }

        fn tag(&self) -> widget::tree::Tag {
            struct Marker;
            widget::tree::Tag::of::<Marker>()
        }

        fn diff(&self, tree: &mut Tree) {
            let instants = tree.state.downcast_mut::<Vec<Option<Instant>>>();
            instants.retain(Option::is_some);

            match (instants.len(), self.toasts.len()) {
                (old, new) if old > new => instants.truncate(new),
                (old, new) if old < new => instants.extend(std::iter::repeat(Some(Instant::now())).take(new - old)),
                _ => {},
            }
            tree.diff_children(&std::iter::once(&self.content).chain(self.toasts.iter()).collect::<Vec<&Element<'a, Message, Theme, Renderer>>>())
        }

        fn operate(&self, state: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn iced::advanced::widget::Operation<Message>) {
            operation.container(None, layout.bounds(), &mut |operation| {
                self.content.as_widget().operate(&mut state.children[0], layout, renderer, operation);
            });
        }

        fn on_event(&mut self, state: &mut Tree, event: iced::Event, layout: Layout<'_>, cursor: iced::advanced::mouse::Cursor, renderer: &Renderer, clipboard: &mut dyn iced::advanced::Clipboard, shell: &mut iced::advanced::Shell<'_, Message>, viewport: &iced::Rectangle) -> iced::advanced::graphics::core::event::Status {
            self.content.as_widget_mut().on_event(&mut state.children[0], event, layout, cursor, renderer, clipboard, shell, viewport)            
        }

        fn mouse_interaction(&self, state: &Tree, layout: Layout<'_>, cursor: iced::advanced::mouse::Cursor, viewport: &iced::Rectangle, renderer: &Renderer) -> iced::advanced::mouse::Interaction {
            self.content.as_widget().mouse_interaction(&state.children[0], layout, cursor, viewport, renderer)
        }

        fn overlay<'b>(&'b mut self, state: &'b mut Tree, layout: Layout<'_>, renderer: &Renderer, translation: iced::Vector) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
            let instants = state.state.downcast_mut::<Vec<Option<Instant>>>();
            let (content_state, toast_state) = state.children.split_at_mut(1);
            let content = self.content.as_widget_mut().overlay(&mut content_state[0], layout, renderer, translation);
            let toasts = (!self.toasts.is_empty()).then(|| {
                overlay::Element::new(Box::new(Overlay {
                    toasts: &mut self.toasts,
                    state: toast_state,
                    instants,
                    on_close: &self.on_close,
                }))
            });
            let overlays = content.into_iter().chain(toasts).collect::<Vec<_>>();
            (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).overlay())
        }
    }

    impl<'a> From<Manager<'a>> for Element<'a, Message, Theme> {
        fn from(manager: Manager<'a>) -> Self {
            Element::new(manager)
        }
    }

    struct Overlay<'a, 'b> {
        toasts: &'b mut [Element<'a, Message, Theme>],
        state: &'b mut [Tree],
        instants: &'b mut [Option<Instant>],
        on_close: &'b dyn Fn(usize) -> Message,
    }

    impl<'a, 'b> overlay::Overlay<Message, Theme, Renderer> for Overlay<'a, 'b> {
        fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
            let limits = layout::Limits::new(Size::ZERO, bounds);
            layout::flex::resolve(layout::flex::Axis::Vertical, renderer, &limits, Length::Fill, Length::Shrink, 10.into(), 10f32, Alignment::End, self.toasts, self.state).align(Alignment::End, Alignment::End, bounds)
        }

        fn on_event(&mut self, event: iced::Event, layout: Layout<'_>, cursor: iced::advanced::mouse::Cursor, renderer: &Renderer, clipboard: &mut dyn iced::advanced::Clipboard, shell: &mut iced::advanced::Shell<'_, Message>) -> iced::advanced::graphics::core::event::Status {
            if let Event::Window(_, window::Event::RedrawRequested(now)) = event {
                let mut next_redraw = None;
                self.instants.iter_mut().enumerate().for_each(|(index, maybe_instant)| {
                    if let Some(instant) = maybe_instant.as_mut() {
                        let remaining = TOAST_DURATION.saturating_sub(instant.elapsed());
                        if let Duration::ZERO = remaining {
                            maybe_instant.take();
                            shell.publish((self.on_close)(index));
                            next_redraw = Some(window::RedrawRequest::NextFrame);
                        }
                        else {
                            let redraw_at = window::RedrawRequest::At(now + remaining);
                            next_redraw = next_redraw.map(|redraw| redraw.min(redraw_at)).or(Some(redraw_at));
                        }
                    }
                });
                if let Some(redraw) = next_redraw {
                    shell.request_redraw(redraw);
                }
            }

            let viewport = layout.bounds();
            self.toasts.iter_mut().zip(self.state.iter_mut()).zip(layout.children()).zip(self.instants.iter_mut()).map(|(((child, state), layout), instant)| {
                let mut local_messages = Vec::new();
                let mut local_shell = Shell::new(&mut local_messages);
                let status = child.as_widget_mut().on_event(state, event.clone(), layout, cursor, renderer, clipboard, &mut local_shell, &viewport);
                if !local_shell.is_empty() {
                    instant.take();
                }
                shell.merge(local_shell, std::convert::identity);
                status
            }).fold(event::Status::Ignored, event::Status::merge)
        }
    
        fn draw(&self, renderer: &mut Renderer, theme: &Theme, style: &iced::advanced::renderer::Style, layout: Layout<'_>, cursor: iced::advanced::mouse::Cursor) {
            let viewport = layout.bounds();

            for ((child, state), layout) in self.toasts.iter().zip(self.state.iter()).zip(layout.children()) {
                child.as_widget().draw(state, renderer, theme, style, layout, cursor, &viewport);
            }
        }

        fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn iced::advanced::widget::Operation<Message>) {
            operation.container(None, layout.bounds(), &mut |operation| {
                self.toasts.iter().zip(self.state.iter_mut()).zip(layout.children()).for_each(|((child, state), layout)| {
                        child.as_widget().operate(state, layout, renderer, operation);
                });
            });
        }

        fn mouse_interaction(&self, layout: Layout<'_>, cursor: iced::advanced::mouse::Cursor, viewport: &iced::Rectangle, renderer: &Renderer) -> iced::advanced::mouse::Interaction {
            self.toasts.iter().zip(self.state.iter()).zip(layout.children()).map(|((child, state), layout)| child.as_widget().mouse_interaction(state, layout, cursor, viewport, renderer)).max().unwrap_or_default()
        }

        fn is_over(&self, layout: Layout<'_>, _renderer: &Renderer, cursor_position: Point) -> bool {
            layout.children().any(|layout| layout.bounds().contains(cursor_position))
        }
    }
}

mod theme {
    use iced::{application, border::Radius, color, widget::{button, container, radio, text, text_editor, text_input}, Background, Border, Color};

    #[derive(Default)]
    pub struct Theme;

    impl Theme {
        const SECONDARY: Color = color!(246, 201, 14);
        const PRIMARY: Color = color!(48, 56, 65);
        const TERTIARY: Color = color!(68, 93, 72);
        const BEIGE: Color = color!(245, 245, 220);
        const PLACEHOLDER: Color = color!(107, 107, 107);
        const SELECTION: Color = color!(181, 94, 2);
        const RED: Color = color!(224, 29, 29);
        const GRAY: Color = color!(202, 207, 203);
        const GREEN: Color = color!(20, 184, 64);
    }

    #[derive(Default)]
    pub enum Container {
        #[default]
        Default,
        Cred,
        InfoToast,
        SuccessToast,
        DangerToast,
    }

    impl container::StyleSheet for Theme {
        type Style = Container;
        
        fn appearance(&self, style: &Self::Style) -> container::Appearance {
            container::Appearance {
                background: match style {
                    Container::Cred => Some(Background::Color(Self::TERTIARY)),
                    Container::InfoToast => Some(Background::Color(Self::SECONDARY)),
                    Container::SuccessToast => Some(Background::Color(Self::GREEN)),
                    Container::DangerToast => Some(Background::Color(Self::RED)),
                    Container::Default => None,
                },
                border: Border {
                    radius: Radius::from(5),
                    ..Default::default()
                },
                ..Default::default()
            }
        }
    }

    impl application::StyleSheet for Theme {
        type Style = ();

        fn appearance(&self, _style: &Self::Style) -> application::Appearance {
            application::Appearance {
                background_color: Self::PRIMARY,
                text_color: Color::BLACK,
            }
        }
    }

    #[derive(Clone, Default)]
    pub enum Text {
        Title,
        #[default]
        Default,
        Black,
        Gray,
        Light,
    }

    impl text::StyleSheet for Theme {
        type Style = Text;

        fn appearance(&self, style: Self::Style) -> text::Appearance {
            text::Appearance {
                color: Some(match style {
                    Text::Title => Self::SECONDARY,
                    Text::Default => Self::PRIMARY,
                    Text::Black => Color::BLACK,
                    Text::Gray => Self::GRAY,
                    Text::Light => Self::BEIGE,
                }),
            }
        }
    }

    #[derive(Default)]
    pub enum Button {
        #[default]
        Default,
        Distinct,
        Cred,
        Delete,
        Edit,
        Toast,
    }

    impl button::StyleSheet for Theme {
        type Style = Button;
        
        fn active(&self, style: &Self::Style) -> button::Appearance {
            button::Appearance {
                background:  match style {
                    Button::Distinct => Some(Background::Color(Self::RED)),
                    Button::Cred | Button::Toast => None,
                    _ => Some(Background::Color(Self::SECONDARY)),
                },
                border: Border {
                    radius: Radius::from(5),
                    ..Default::default()
                },
                ..Default::default()
            }
        }
    }

    impl text_input::StyleSheet for Theme {
        type Style = ();
        
        fn active(&self, _style: &Self::Style) -> text_input::Appearance {
            text_input::Appearance { 
                background: Background::Color(Self::BEIGE),
                border: Border {
                    radius: Radius::from(5),
                    ..Default::default()
                },
                icon_color: color!(84, 84, 84),
             }
        }
        
        fn focused(&self, style: &Self::Style) -> text_input::Appearance {
            self.active(style)
        }
        
        fn placeholder_color(&self, _style: &Self::Style) -> iced::Color {
            Self::PLACEHOLDER
        }
        
        fn value_color(&self, _style: &Self::Style) -> iced::Color {
            Color::BLACK
        }
        
        fn disabled_color(&self, _style: &Self::Style) -> iced::Color {
            Self::SECONDARY
        }
        
        fn selection_color(&self, _style: &Self::Style) -> iced::Color {
            Self::SELECTION    
        }
        
        fn disabled(&self, style: &Self::Style) -> text_input::Appearance {
            self.active(style)
        }
    }

    impl text_editor::StyleSheet for Theme {
        type Style = ();
        
        fn active(&self, _style: &Self::Style) -> text_editor::Appearance {
            text_editor::Appearance { 
                background: Background::Color(Self::BEIGE),
                border: Border { 
                    radius: Radius::from(5),
                    ..Default::default()
                 }
            }
        }
        
        fn focused(&self, style: &Self::Style) -> text_editor::Appearance {
            self.active(style)
        }
        
        fn placeholder_color(&self, _style: &Self::Style) -> Color {
            Self::PLACEHOLDER
        }
        
        fn value_color(&self, _style: &Self::Style) -> Color {
            Color::BLACK
        }
        
        fn disabled_color(&self, _style: &Self::Style) -> Color {
            Self::SECONDARY
        }
        
        fn selection_color(&self, _style: &Self::Style) -> Color {
            Self::SELECTION
        }
        
        fn disabled(&self, style: &Self::Style) -> text_editor::Appearance {
            self.active(style)
        }
    }

    impl radio::StyleSheet for Theme {
        type Style = ();
        
        fn active(&self, _style: &Self::Style, _is_selected: bool) -> radio::Appearance {
            radio::Appearance {
                background: Background::Color(Self::SECONDARY),
                dot_color: Self::PRIMARY,
                border_width: 0f32,
                border_color: Self::SECONDARY,
                text_color: Some(Self::SECONDARY),
             }
        }
        
        fn hovered(&self, style: &Self::Style, is_selected: bool) -> radio::Appearance {
            self.active(style, is_selected)
        }

    }
}

mod core {
    use std::{fs, io::{self, Write}, sync::Arc};
    use aes::{cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit}, Aes128};
    // use block_padding::{Padding, Pkcs7};
    use pbkdf2::pbkdf2_hmac_array;
    use crate::Secret;

    #[derive(Debug)]
    pub struct Storage {
        cipher: Aes128,
        file_name: String,
        directory: String,
    }

    impl Storage {
        pub fn new_from_secret(secret: &str) -> Self {
            const SALT: &str = "salt";
            const N: u32 = 100_000;
            let key = pbkdf2_hmac_array::<sha2::Sha256, 16>(secret.as_bytes(), SALT.as_bytes(), N);
            let cipher = Aes128::new(&GenericArray::from(key));
            let file_name = "store.aes".to_string();
            let directory = ".store".to_string();
            Self {
                cipher,
                file_name,
                directory,
            }
        }
    }

    #[derive(Debug)]
    pub enum StoreError {
        IO(io::Error),
        PadError,
        UnpadError,
    }

    impl From<StoreError> for String {
        fn from(error: StoreError) -> Self {
            match error {
                StoreError::IO(io_error) => io_error.to_string(),
                StoreError::PadError => String::from("error while padding"),
                StoreError::UnpadError => String::from("error while unpadding"),
            }
        }
    }

    pub async fn save(storage: Arc<Storage>, creds: Vec<(String, Secret, String)>) -> Result<(), StoreError> {
        let content = creds.into_iter().map(|(username, secret, description)| {
            format!("{},{},{}", username, secret, description)
        }).collect::<Vec::<String>>().join("\n");

        let mut buffer = Vec::new();
        for chunk in content.as_bytes().chunks(16) {
            let mut block_array: GenericArray<u8, _> = {
                let mut padded_array = [0xff; 16];
                let padded_block = Pkcs7::pad(chunk, 16)?;
                padded_array.copy_from_slice(padded_block.as_slice());
                GenericArray::from(padded_array)
            };
            storage.cipher.encrypt_block(&mut block_array);
            buffer.extend_from_slice(block_array.as_slice());
        }

        fs::create_dir_all(&storage.directory).map_err(StoreError::IO)?;
        let mut file = fs::File::create(format!("{}/{}", storage.directory, storage.file_name)).map_err(StoreError::IO)?;
        file.write_all(buffer.as_slice()).map_err(StoreError::IO)?;
        Ok(())
    }

    pub async fn fetch(storage: Arc<Storage>) -> Result<Vec<(String, Secret, String)>, StoreError> {
        const DIR: &str = ".store";
        let buffer = fs::read(format!("{}/{}", DIR, storage.file_name)).map_err(StoreError::IO)?;
        let mut decrypted_buffer: Vec<u8> = Vec::new();
        for chunk in buffer.chunks(16) {
            if chunk.len() < 16 {
                return Err(StoreError::PadError);
            }
            let mut block_array = GenericArray::from_slice(chunk).to_owned();
            storage.cipher.decrypt_block(&mut block_array);
            let content = Pkcs7::unpad(&block_array)?;
            decrypted_buffer.extend_from_slice(content);
        }
        let content = String::from_utf8_lossy(decrypted_buffer.as_slice());
        let content = content.lines().map(|buffer| {
            let mut iter = buffer.split(',');
            let username = iter.next().unwrap().to_owned();
            let secret = Secret::from(iter.next().unwrap());
            let description = iter.next().unwrap_or_default().to_owned();
            (username, secret, description)
        }).collect::<Vec::<(String, Secret, String)>>();
        Ok(content)
    }

    struct Pkcs7;

    impl Pkcs7 {
        fn pad(block: &[u8], len: usize) -> Result<Vec<u8>, StoreError> {
            if block.len() > 255 || len < block.len() {
                Err(StoreError::PadError)
            }
            else {
                let n = len - block.len();
                let padded_block = block.iter().chain(std::iter::repeat(&(n as u8)).take(n)).map(u8::to_owned).collect::<Vec::<u8>>();
                Ok(padded_block)
            }
        }


        fn unpad(block: &[u8]) -> Result<&[u8], StoreError> {
            let n = block.last().ok_or(StoreError::UnpadError)?;
            if block.len() > u8::MAX as usize || *n == 0 || *n as usize >= block.len() {
                Ok(block)
            }
            else {
                let s = block.len() - *n as usize;
                if block[s..].iter().any(|&v| v != *n) {
                    Ok(block)
                }
                else {
                    Ok(&block[..s])
                }
            }
        }
    }

    #[derive(Clone, Debug)]
    pub enum StoreMessage {
        Fetched(Vec<(String, Secret, String)>),
        Added,
        Deleted,
        Updated,
        Invalid,
    }
    
    #[test]
    fn unpad_empty_block() {
        let arr: &[u8] = &[];
        let res = Pkcs7::unpad(arr);
        assert!(res.is_err());
    }

    #[test]
    fn unpad_block_with_length_greater_than_255() {
        let arr = [2u8; 500];
        let res = Pkcs7::unpad(&arr[..]);
        assert!(res.is_ok_and(|unpadded_arr| unpadded_arr == arr));
    }

    #[test]
    fn unpad_block_with_last_byte_0() {
        let arr = (0..100).rev().collect::<Vec<u8>>();
        let res = Pkcs7::unpad(arr.as_slice());
        assert!(res.is_ok_and(|unpadded_arr| unpadded_arr == arr));
    }

    #[test]
    fn unpad_block_with_last_byte_greater_than_length_of_array() {
        let arr = [50u8, 51, 52];
        let res = Pkcs7::unpad(&arr[..]);
        assert!(res.is_ok_and(|unpadded_arr| unpadded_arr == &arr[..]));
    }

    #[test]
    fn unpad_block_with_bytes_ne_to_unpad_length() {
        let arr = [9, 8, 7, 6, 5, 4, 3, 10, 10, 10, 10, 10, 10, 3, 10, 10, 10];
        let res = Pkcs7::unpad(&arr[..]);
        assert!(res.is_ok_and(|unpadded_arr| unpadded_arr == &arr[..]));
    }
}