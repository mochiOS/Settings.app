//! Settings owns the system crash windows; all data and presentation contracts
//! come from AppCore. IPC and report reads never run on the UI thread.
use super::*;
use appcore::{
    Alert, SavePanel, SavePanelOptions,
    diagnostics::{self as api, CrashReport, CrashSummary},
};
use std::sync::mpsc::{self, Receiver, Sender};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Prompt,
    History,
    Details,
}
#[derive(Clone)]
struct Model {
    page: Page,
    rows: Vec<CrashSummary>,
    report: Option<CrashReport>,
    loading: bool,
    error: String,
    count: usize,
    ticket: u64,
}
enum Event {
    History(u64, Result<Vec<CrashSummary>, String>),
    Report(u64, Result<CrashReport, String>),
    Reopened(Result<(), String>),
}
struct Inner {
    model: State<Model>,
    receiver: RefCell<Receiver<Event>>,
    sender: Sender<Event>,
    alert: Alert,
    panel: RefCell<Option<SavePanel>>,
    scroll: ScrollState,
}
#[derive(Clone)]
pub(super) struct Controller(Rc<Inner>);
impl Controller {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self(Rc::new(Inner {
            model: State::new(Model {
                page: Page::History,
                rows: Vec::new(),
                report: None,
                loading: false,
                error: String::new(),
                count: 1,
                ticket: 0,
            }),
            receiver: RefCell::new(receiver),
            sender,
            alert: Alert::new(),
            panel: RefCell::new(None),
            scroll: ScrollState::new(),
        }))
    }
    fn spawn(&self, operation: impl FnOnce() -> Event + Send + 'static) {
        let sender = self.0.sender.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("diagnostics-read".into())
            .spawn(move || {
                let _ = sender.send(operation());
            })
        {
            let mut model = self.0.model.get();
            model.loading = false;
            model.error = format!("Unable to start diagnostic operation: {error}");
            self.0.model.set(model);
        }
    }
    pub fn history(&self) {
        let mut model = self.0.model.get();
        model.page = Page::History;
        model.loading = true;
        model.error.clear();
        model.ticket += 1;
        let ticket = model.ticket;
        self.0.model.set(model);
        self.spawn(move || Event::History(ticket, api::history().map_err(|e| e.to_string())));
    }
    fn report(&self, id: String, prompt: bool, count: usize) {
        let mut model = self.0.model.get();
        model.page = if prompt { Page::Prompt } else { Page::Details };
        model.report = None;
        model.loading = true;
        model.count = count.max(1);
        model.error.clear();
        model.ticket += 1;
        let ticket = model.ticket;
        self.0.model.set(model);
        self.spawn(move || Event::Report(ticket, api::report(&id).map_err(|e| e.to_string())));
    }
    fn reopen(&self, report: CrashReport) {
        let mut model = self.0.model.get();
        model.loading = true;
        self.0.model.set(model);
        self.spawn(move || Event::Reopened(api::reopen(&report).map_err(|e| e.to_string())));
    }
    fn export(&self, report: CrashReport) {
        let panel = SavePanel::new(
            SavePanelOptions {
                title: "Export Crash Report".into(),
                suggested_name: report.summary.id.clone(),
                ..SavePanelOptions::default()
            },
            move |path| {
                report
                    .export(path)
                    .map_err(|e| format!("Unable to export report: {e}"))
            },
        );
        panel.show();
        *self.0.panel.borrow_mut() = Some(panel);
    }
    fn poll(&self) {
        while let Ok(event) = self.0.receiver.borrow_mut().try_recv() {
            let mut model = self.0.model.get();
            match event {
                Event::History(ticket, result) if ticket == model.ticket => {
                    model.loading = false;
                    match result {
                        Ok(rows) => model.rows = rows,
                        Err(e) => model.error = e,
                    }
                }
                Event::Report(ticket, result) if ticket == model.ticket => {
                    model.loading = false;
                    match result {
                        Ok(report) => model.report = Some(report),
                        Err(e) => model.error = e,
                    }
                }
                Event::Reopened(result) => {
                    model.loading = false;
                    match result {
                        Ok(()) if model.page == Page::Prompt => viewkit::request_close_key_window(),
                        Ok(()) => {}
                        Err(e) => self
                            .0
                            .alert
                            .present_error("The application could not be reopened.", e),
                    }
                }
                _ => continue,
            }
            self.0.model.set(model);
        }
    }
    pub fn window(&self) -> WindowOptions {
        WindowOptions::new("Crash Reports")
            .compact()
            .resizable(true)
    }
    pub fn view(&self) -> Box<dyn View> {
        let model = self.0.model.get();
        let content: Box<dyn View> = if model.page == Page::Prompt
            && model.report.is_some()
            && !model.loading
        {
            let report = model.report.clone().unwrap();
            let reopen = self.clone();
            let detail = self.clone();
            api::crash_dialog(
                &report.clone(),
                model.count,
                move || reopen.reopen(report.clone()),
                move || {
                    let mut m = detail.0.model.get();
                    m.page = Page::Details;
                    detail.0.model.set(m);
                },
                viewkit::request_close_key_window,
            )
        } else {
            let history = self.clone();
            let refresh = self.clone();
            let mut header = HStack::new()
                .child(Text::new("Crash Reports").style(TextRole::TitleSmall))
                .child(Spacer::new())
                .child(
                    Button::new("History")
                        .enabled(!model.loading)
                        .on_click(move || history.history()),
                )
                .child(
                    Button::new("Refresh")
                        .enabled(!model.loading)
                        .on_click(move || refresh.history()),
                );
            if let Some(report) = model
                .report
                .as_ref()
                .filter(|_| model.page == Page::Details)
            {
                let reopen = self.clone();
                let export = self.clone();
                let reopen_report = report.clone();
                let export_report = report.clone();
                header = header
                    .child(
                        Button::new("Reopen")
                            .enabled(!model.loading && report.summary.package_id.is_some())
                            .on_click(move || reopen.reopen(reopen_report.clone())),
                    )
                    .child(
                        Button::new("Export…")
                            .enabled(!model.loading)
                            .on_click(move || export.export(export_report.clone())),
                    );
            }
            let mut body = VStack::new()
                .gap(StackGap::Large)
                .alignment(StackAlignment::Stretch);
            if model.loading {
                body = body.child(Text::new("Loading…").tone(TextTone::Secondary));
            }
            if !model.error.is_empty() {
                body = body.child(Text::new(format!(
                    "Unable to load diagnostic reports: {}",
                    model.error
                )));
            } else if model.page == Page::History && !model.loading {
                if model.rows.is_empty() {
                    body =
                        body.child(Text::new("No local crash reports.").tone(TextTone::Secondary));
                }
                for report in &model.rows {
                    let detail = self.clone();
                    let id = report.id.clone();
                    body = body.child(
                        Button::new(report.name.clone())
                            .style(ButtonStyle::Ghost)
                            .content(
                                VStack::new()
                                    .gap(StackGap::Small)
                                    .child(
                                        Text::new(report.name.clone()).style(TextRole::TitleSmall),
                                    )
                                    .child(
                                        Text::new(format!(
                                            "{} · {}",
                                            report.version,
                                            time_label(report.utc_seconds)
                                        ))
                                        .tone(TextTone::Secondary),
                                    ),
                            )
                            .on_click(move || detail.report(id.clone(), false, 1)),
                    );
                }
            } else if let Some(report) = &model.report {
                body = body
                    .child(Text::new(report.summary.name.clone()).style(TextRole::TitleSmall))
                    .child(
                        Text::new(time_label(report.summary.utc_seconds)).tone(TextTone::Secondary),
                    )
                    .child(Text::new(report.details.clone()).monospaced());
            }
            Box::new(Padding::all(Theme::current().spacing.extra_large).content(
                VStack::new().alignment(StackAlignment::Stretch).gap(StackGap::Large)
                    .child(header)
                    .child(Text::new("Local reports are kept on this device. Exporting does not send them to mochiOS.")
                        .tone(TextTone::Secondary))
                    .child(Scroll::new(self.0.scroll.clone()).axis(ScrollAxis::Vertical).content(body)
                        .layout().flex_grow(1.0).flex_shrink(1.0))))
        };
        Box::new(PollLayer {
            content,
            controller: self.clone(),
        })
    }
}

fn time_label(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return "Time unavailable".into();
    };
    let Some((year, month, day)) = i64::try_from(seconds / 86_400)
        .ok()
        .and_then(super::civil_date)
    else {
        return "Time unavailable".into();
    };
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

struct PollLayer {
    content: Box<dyn View>,
    controller: Controller,
}
impl View for PollLayer {
    fn measure(&self, constraints: Constraints, context: &mut MeasureContext<'_>) -> Size {
        self.content.measure(constraints, context)
    }
    fn paint(&self, bounds: Rect, context: &mut PaintContext<'_>) {
        self.controller.poll();
        self.content.paint(bounds, context);
        if self.controller.0.model.get().loading {
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

pub(super) fn requested() -> bool {
    std::env::args().any(|arg| arg == "--diagnostics" || arg.starts_with("--show-crash="))
}
pub(super) struct CrashApp {
    controller: Controller,
}
impl App for CrashApp {
    type Body = Box<dyn View>;
    fn new() -> Self {
        let controller = Controller::new();
        let id =
            std::env::args().find_map(|arg| arg.strip_prefix("--show-crash=").map(str::to_owned));
        let count = std::env::args()
            .find_map(|arg| {
                arg.strip_prefix("--crash-count=")
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .unwrap_or(1)
            .clamp(1, 128);
        if let Some(id) = id {
            controller.report(id, true, count);
        } else {
            controller.history();
        }
        Self { controller }
    }
    fn window(&self) -> WindowOptions {
        self.controller.window()
    }
    fn body(&self, _context: &ViewContext) -> Self::Body {
        self.controller.view()
    }
}

pub(super) fn consent_status() -> Vec<(&'static str, String)> {
    use mochios_device_consent::{ConsentStore, DataCategory};
    let record = ConsentStore::system().load();
    [
        ("Usage Diagnostics", DataCategory::UsageDiagnostics),
        ("Crash Report Consent", DataCategory::CrashReports),
        ("Update Results", DataCategory::UpdateResults),
    ]
    .into_iter()
    .map(|(label, category)| {
        (
            label,
            match &record {
                Ok(record) if record.allows(category, category.policy_version()) => {
                    "Allowed".into()
                }
                Ok(_) => "Not allowed".into(),
                Err(_) => "Unable to read consent".into(),
            },
        )
    })
    .collect()
}
