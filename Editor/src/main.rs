use iced::{ Element, Settings, Subscription, Length, theme, widget::{column, row, text, button, scrollable, container}};
use iced::widget::text_input::TextInput; // use text input as a simple editor replacement
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender, Receiver};
use std::time::Duration;
use std::process::Command;
use std::io::Write;

fn main() -> iced::Result {
    EditorApp::run(Settings::default());

#[derive(Debug, Clone)]
enum Message {
    EditorChanged(String),
    Assemble,
    Run,
    Output(String),
    Tick,
}

struct EditorApp {
    editor_content: String,
    console_output: String,
    is_busy: bool,
    tx: Sender<String>,
    rx: Receiver<String>,
}
impl  EditorApp {
    type Message = Message;

    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        EditorApp {
            editor_content: String::new(),
            console_output: String::new(),
            is_busy: false,
            tx,
            rx,
        }
    }

    fn title(&self) -> String {
        "MicroV2 Minimal Editor".to_string()
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::EditorChanged(s) => self.editor_content = s,
            Message::Assemble => {
                let code = self.editor_content.clone();
                let tx = self.tx.clone();
                self.is_busy = true;
                std::thread::spawn(move || {
                    let out = run_masm_on_sync(code);
                    let _ = tx.send(out);
                });
            }
            Message::Run => {
                let code = self.editor_content.clone();
                let tx = self.tx.clone();
                self.is_busy = true;
                std::thread::spawn(move || {
                    let out = run_masm_on_sync(code);
                    let _ = tx.send(out);
                });
            }
            Message::Output(s) => {
                self.console_output = s;
                self.is_busy = false;
            }
            Message::Tick => {
                // drain any available messages from channel
                while let Ok(s) = self.rx.try_recv() {
                    self.console_output = s;
                    self.is_busy = false;
                }
            }
        }
    }

    fn view(&self) -> Element<Message> {
        let editor = TextInput::new( &self.editor_content)
            .on_input(Message::EditorChanged)
            .padding(10)
            .size(16)
            .width(Length::Fill);

        let console = container(
            scrollable(text(&self.console_output).size(16))
        )
        .padding(10)
        .height(Length::Units(160));

        let assemble_btn = button(text("Assemble")).on_press(Message::Assemble);
        let run_btn = button(text("Run")).on_press(Message::Run);

        column![
            row![assemble_btn, run_btn].spacing(10).padding(10),
            editor.height(Length::Fill),
            text("Console Output:").size(18),
            console,
        ]
        .spacing(10)
        .padding(10)
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick)
    }
}

async fn run_assemble(code: String) -> String {
    // keep for compatibility but unused in sandbox flow
    let _ = code;
    "[Assemble] Use Assemble button in UI".to_string()
}

async fn run_run(code: String) -> String {
    let _ = code;
    "[Run] Use Run button in UI".to_string()
}

fn run_masm_on_sync(code: String) -> String {
    let tmp_path = PathBuf::from("temp.masm");
    if let Err(e) = fs::write(&tmp_path, &code) {
        return format!("Save error: {}", e);
    }

    // Try to run masm binary if present, otherwise fallback to cargo run
    let manifest = "..\\masm\\Cargo.toml";
    let output = Command::new("cargo")
        .args(&["run", "--manifest-path", manifest, "--", &tmp_path.to_string_lossy()])
        .output();

    match output {
        Ok(output) => {
            let mut out = String::new();
            out.push_str("--- stdout ---\n");
            out.push_str(&String::from_utf8_lossy(&output.stdout));
            out.push_str("\n--- stderr ---\n");
            out.push_str(&String::from_utf8_lossy(&output.stderr));
            out.push_str(&format!("\nstatus: {}\n", output.status));
            out
        }
        Err(e) => format!("failed to spawn cargo: {}", e),
    }
}
}
