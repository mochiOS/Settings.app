//! Device-wide data sharing preferences. Protected writes use the existing
//! administrator-operation broker; this app never replaces the system record.
use super::*;
use mochios_device_consent::{ConsentRecord, ConsentStore, DataCategory, documents::Documents};
use std::sync::mpsc::{self, Receiver, Sender};

const TITLES: [&str; 3] = ["Usage & Diagnostics", "Crash Reports", "Update Results"];
#[derive(Clone)]
struct Model {
    record: ConsentRecord,
    busy: bool,
    message: String,
}
struct Inner {
    documents: Result<Documents, String>,
    model: State<Model>,
    choices: [State<usize>; 3],
    interactions: [SegmentedControlInteractionState; 3],
    legal_confirmed: State<bool>,
    legal_interaction: SwitchInteractionState,
    details: State<bool>,
    receiver: RefCell<Receiver<Result<ConsentRecord, String>>>,
    sender: Sender<Result<ConsentRecord, String>>,
}
#[derive(Clone)]
pub(super) struct Controller(Rc<Inner>);
impl Controller {
    pub fn new() -> Self {
        #[cfg(target_os = "mochios")]
        let root = std::path::PathBuf::from("/system/resources/legal");
        #[cfg(not(target_os = "mochios"))]
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../installer/legal");
        let documents =
            Documents::load(&root).map_err(|_| "Privacy documents could not be loaded.".to_owned());
        #[cfg(target_os = "mochios")]
        let loaded = ConsentStore::system().load();
        #[cfg(not(target_os = "mochios"))]
        let loaded: std::io::Result<ConsentRecord> = Ok(ConsentRecord::default());
        let (record, message) = match loaded {
            Ok(record) => (record, String::new()),
            Err(_) => (
                ConsentRecord::default(),
                "Device privacy settings could not be read.".into(),
            ),
        };
        let choices = selected_choices(&record).map(State::new);
        let (sender, receiver) = mpsc::channel();
        Self(Rc::new(Inner {
            documents,
            model: State::new(Model {
                record,
                busy: false,
                message,
            }),
            choices,
            interactions: std::array::from_fn(|_| SegmentedControlInteractionState::new()),
            legal_confirmed: State::new(false),
            legal_interaction: SwitchInteractionState::new(),
            details: State::new(false),
            receiver: RefCell::new(receiver),
            sender,
        }))
    }
    fn save(&self) {
        let Ok(documents) = &self.0.documents else {
            return;
        };
        let mut model = self.0.model.get();
        if model.busy {
            return;
        }
        let values = self.0.choices.each_ref().map(|s| s.get());
        if values.iter().any(|v| *v > 1) {
            return;
        }
        let choices = values.map(|v| v == 1);
        let legal = self.0.legal_confirmed.get();
        let digest = documents.bundle_hash();
        if !documents.legal_matches(&model.record) && !legal {
            return;
        }
        model.busy = true;
        model.message = "Waiting for administrator approval…".into();
        self.0.model.set(model);
        let sender = self.0.sender.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("privacy-settings".into())
            .spawn(move || {
                let _ = sender.send(apply(choices, legal, digest));
            })
        {
            let mut model = self.0.model.get();
            model.busy = false;
            model.message = format!("Could not start privacy settings operation: {error}");
            self.0.model.set(model);
        }
    }
    fn poll(&self) {
        while let Ok(result) = self.0.receiver.borrow_mut().try_recv() {
            let mut model = self.0.model.get();
            model.busy = false;
            match result {
                Ok(record) => {
                    model.record = record;
                    model.message = "Data sharing preferences saved.".into();
                    self.0.legal_confirmed.set(false);
                }
                Err(message) => model.message = message,
            }
            self.0.model.set(model);
        }
    }
    pub fn view(&self) -> Box<dyn View> {
        let model = self.0.model.get();
        let mut body = VStack::new()
            .alignment(StackAlignment::Stretch)
            .gap(StackGap::Medium);
        let mut legal_ready = false;
        let mut changed = false;
        if let Ok(documents) = &self.0.documents {
            legal_ready = documents.legal_matches(&model.record);
            if !legal_ready {
                let licenses = documents
                    .catalog
                    .licenses
                    .iter()
                    .map(|l| {
                        format!(
                            "{} — {}\n{}\n{}",
                            l.name,
                            l.license,
                            documents.text(&l.copyright_file),
                            documents.text(&l.body_file)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let text = format!(
                    "Terms of Use\n{}\n\nPrivacy Policy\n{}\n\nOpen Source Licenses\n{}",
                    documents.text("terms.txt"),
                    documents.text("privacy.txt"),
                    licenses
                );
                body = body.child(Scroll::vertical(Text::body(text)).height(180.0));
                body = body.child(SettingsRow::new("Confirm Legal Documents", Switch::with_interaction(self.0.legal_confirmed.binding(), self.0.legal_interaction.clone()).enabled(!model.busy)).description("I agree to the Terms of Use and confirm that I have reviewed the Privacy Policy and open source licenses."));
            }
            for (index, category) in DataCategory::ALL.into_iter().enumerate() {
                let disclosure = documents.disclosure(category, "en");
                let choice = self.0.choices[index].get();
                changed |= model.record.permission(category).is_none_or(|p| {
                    usize::from(p.agreed()) != choice
                        || p.policy_version != category.policy_version()
                });
                let control = SegmentedControl::with_interaction(
                    self.0.choices[index].binding(),
                    self.0.interactions[index].clone(),
                )
                .disabled_item(2, "Choose…")
                .item(0, "Don't Allow")
                .item(1, "Allow")
                .enabled(!model.busy);
                body = body.child(
                    SettingsRow::new(TITLES[index], control)
                        .description(documents.text(&disclosure.summary_file)),
                );
                if self.0.details.get() {
                    let text = disclosure
                        .fields
                        .iter()
                        .map(|f| format!("{}\n{}", f.title, documents.text(&f.file)))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    body = body.child(Text::body(text));
                }
            }
        } else if let Err(message) = &self.0.documents {
            body = body.child(Text::body(message.clone()));
        }
        body = body.child(Text::caption("These choices apply to this device. Changes take effect only after you save. Crash uploading is not connected in this version."));
        let details = self.0.details.clone();
        let save = self.clone();
        let ready = self.0.documents.is_ok()
            && (legal_ready || self.0.legal_confirmed.get())
            && self.0.choices.iter().all(|s| s.get() < 2)
            && (changed || !legal_ready)
            && !model.busy
            && cfg!(target_os = "mochios");
        body = body.child(
            HStack::new()
                .gap(StackGap::Small)
                .child(
                    Button::new(if self.0.details.get() {
                        "Hide Details"
                    } else {
                        "Show Details"
                    })
                    .style(ButtonStyle::Ghost)
                    .on_click(move || details.set(!details.get())),
                )
                .child(
                    Button::new(if model.busy {
                        "Saving…"
                    } else {
                        "Save Choices…"
                    })
                    .style(ButtonStyle::Accent)
                    .enabled(ready)
                    .on_click(move || save.save()),
                ),
        );
        if !cfg!(target_os = "mochios") {
            body = body.child(Text::caption(
                "Saving device privacy choices is available on mochiOS.",
            ));
        }
        if !model.message.is_empty() {
            body = body.child(Text::body(model.message));
        }
        Box::new(PollLayer {
            content: Box::new(body),
            controller: self.clone(),
        })
    }
}
#[cfg(target_os = "mochios")]
fn apply(choices: [bool; 3], legal: bool, digest: [u8; 32]) -> Result<ConsentRecord, String> {
    let mut bytes = [0; 40];
    let length = mochi_user_platform::capability::administrator::encode_device_privacy(
        choices, legal, &digest, &mut bytes,
    )
    .map_err(|_| "Invalid privacy operation.".to_owned())?;
    mochi_user_platform::administration::perform(&bytes[..length]).map_err(|error| match error
        .errno()
    {
        Some(13) => "Administrator approval was not granted. No changes were saved.".to_owned(),
        Some(11) => "Another protected operation is in progress. Try again.".to_owned(),
        _ => format!(
            "Data sharing preferences could not be saved (error {}).",
            error.raw()
        ),
    })?;
    ConsentStore::system()
        .load()
        .map_err(|_| "Saved privacy settings could not be read.".into())
}
#[cfg(not(target_os = "mochios"))]
fn apply(_: [bool; 3], _: bool, _: [u8; 32]) -> Result<ConsentRecord, String> {
    Err("Device privacy changes require mochiOS administrator authentication.".into())
}
struct PollLayer {
    content: Box<dyn View>,
    controller: Controller,
}
impl View for PollLayer {
    fn measure(&self, c: Constraints, context: &mut MeasureContext<'_>) -> Size {
        self.content.measure(c, context)
    }
    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        self.controller.poll();
        self.content.paint(bounds, context);
        if self.controller.0.model.get().busy {
            context.request_redraw_in_at(bounds, Instant::now() + Duration::from_millis(50));
        }
    }
    fn handle_event(
        &self,
        bounds: Rect,
        event: &ViewEvent,
        context: &mut EventContext<'_>,
    ) -> EventResult {
        self.content.handle_event(bounds, event, context)
    }
}

fn selected_choices(record: &ConsentRecord) -> [usize; 3] {
    DataCategory::ALL.map(|category| {
        record
            .permission(category)
            .filter(|permission| permission.policy_version == category.policy_version())
            .map_or(2, |permission| usize::from(permission.agreed()))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_outdated_permission_requires_a_new_explicit_choice() {
        let mut record = ConsentRecord::default();
        assert_eq!(selected_choices(&record), [2; 3]);
        record.set_permission(
            DataCategory::UsageDiagnostics,
            true,
            "placeholder-2",
            1_800_000_000,
        );
        record.set_permission(
            DataCategory::CrashReports,
            false,
            "placeholder-2",
            1_800_000_000,
        );
        record.set_permission(
            DataCategory::UpdateResults,
            true,
            "old-policy",
            1_800_000_000,
        );
        assert_eq!(selected_choices(&record), [1, 0, 2]);
    }
}
