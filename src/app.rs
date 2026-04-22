use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use anyhow::Result;
use egui::{Align, Layout, RichText, ScrollArea};
use sqlx::SqlitePool;
use tokio::runtime::Handle;
use tokio::sync::watch;

use crate::aggregator::AppEvent;
use crate::autostart;
use crate::tracker::WindowInfo;
use crate::tray::TrayCommand;

#[derive(PartialEq)]
enum Tab {
    Stats,
    History,
    Manage,
}

#[derive(PartialEq, Clone, Copy)]
enum HistoryMode {
    Daily,
    Monthly,
}

#[derive(Clone)]
struct DayRow {
    date: String,
    process: String,
    total_secs: i64,
}

#[derive(Clone)]
struct MonthRow {
    month: String,
    process: String,
    total_secs: i64,
}

pub struct ChronographApp {
    pool: SqlitePool,
    rt: Handle,
    event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    tray_rx: mpsc::Receiver<TrayCommand>,
    active_rx: watch::Receiver<Option<WindowInfo>>,
    paused: bool,

    tab: Tab,

    // 오늘 탭
    accumulated: HashMap<String, i64>,
    current_window: Option<WindowInfo>,
    session_start: Option<Instant>,
    last_db_refresh: Instant,

    // 기록 탭
    history_mode: HistoryMode,
    daily_rows: Vec<DayRow>,
    monthly_rows: Vec<MonthRow>,
    history_loaded: bool,

    // 앱 관리: DB에 기록된 프로세스 목록 + 표시 여부
    known_processes: Vec<String>,       // DB에서 조회한 프로세스명 전체
    visible_processes: HashSet<String>, // 표시할 프로세스
    manage_loaded: bool,
}

impl ChronographApp {
    pub fn new(
        pool: SqlitePool,
        rt: Handle,
        event_tx: tokio::sync::mpsc::Sender<AppEvent>,
        active_rx: watch::Receiver<Option<WindowInfo>>,
        tray_rx: mpsc::Receiver<TrayCommand>,
        ctx_slot: Arc<Mutex<Option<egui::Context>>>,
        cc: &eframe::CreationContext<'_>,
    ) -> Self {
        load_korean_font(&cc.egui_ctx);
        *ctx_slot.lock().unwrap() = Some(cc.egui_ctx.clone());

        let accumulated = rt.block_on(load_today_accumulated(&pool));

        Self {
            pool,
            rt,
            event_tx,
            tray_rx,
            active_rx,
            paused: false,
            tab: Tab::Stats,
            accumulated,
            current_window: None,
            session_start: None,
            last_db_refresh: Instant::now(),
            history_mode: HistoryMode::Daily,
            daily_rows: Vec::new(),
            monthly_rows: Vec::new(),
            history_loaded: false,
            known_processes: Vec::new(),
            visible_processes: HashSet::new(),
            manage_loaded: false,
        }
    }

    fn refresh_from_db(&mut self) {
        self.accumulated = self.rt.block_on(load_today_accumulated(&self.pool));
        self.last_db_refresh = Instant::now();
    }

    fn load_history(&mut self) {
        let (daily, monthly) = self.rt.block_on(load_history_data(&self.pool));
        self.daily_rows = daily;
        self.monthly_rows = monthly;
        self.history_loaded = true;
    }

    fn load_known_processes(&mut self) {
        let procs = self.rt.block_on(load_all_processes(&self.pool));
        // 새로 발견된 프로세스는 자동으로 표시 목록에 추가
        for p in &procs {
            self.visible_processes.insert(p.clone());
        }
        self.known_processes = procs;
        self.manage_loaded = true;
    }

    fn display_totals(&self) -> Vec<(String, i64)> {
        let mut totals: HashMap<String, i64> = self.accumulated.clone();
        if let (Some(win), Some(start)) = (&self.current_window, &self.session_start) {
            let elapsed = start.elapsed().as_secs() as i64;
            *totals.entry(win.process_name.clone()).or_insert(0) += elapsed;
        }
        let mut list: Vec<(String, i64)> = totals
            .into_iter()
            .filter(|(_, secs)| *secs >= 1)
            .collect();
        list.sort_by(|a, b| b.1.cmp(&a.1));
        list
    }
}

async fn load_today_accumulated(pool: &SqlitePool) -> HashMap<String, i64> {
    let now = chrono::Local::now();
    let midnight = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap();
    let day_start_ms = midnight.timestamp_millis();

    sqlx::query_as::<_, (String, i64)>(
        "SELECT process_name, SUM(end_ts - start_ts) / 1000
         FROM sessions
         WHERE start_ts >= ? AND is_idle = 0 AND end_ts IS NOT NULL
         GROUP BY process_name
         HAVING SUM(end_ts - start_ts) >= 1000",
    )
    .bind(day_start_ms)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .collect()
}

async fn load_all_processes(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_as::<_, (String,)>(
        "SELECT DISTINCT process_name FROM sessions WHERE is_idle = 0 ORDER BY process_name",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(p,)| p)
    .collect()
}

async fn load_history_data(pool: &SqlitePool) -> (Vec<DayRow>, Vec<MonthRow>) {
    let daily = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT date(start_ts / 1000, 'unixepoch', 'localtime'),
                process_name,
                SUM(COALESCE(end_ts, last_heartbeat_ts) - start_ts) / 1000
         FROM sessions
         WHERE is_idle = 0
           AND start_ts >= (strftime('%s', 'now', '-30 days') * 1000)
         GROUP BY 1, process_name
         HAVING SUM(COALESCE(end_ts, last_heartbeat_ts) - start_ts) >= 1000
         ORDER BY 1 DESC, 3 DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(date, process, secs)| DayRow { date, process, total_secs: secs })
    .collect();

    let monthly = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT strftime('%Y-%m', start_ts / 1000, 'unixepoch', 'localtime'),
                process_name,
                SUM(COALESCE(end_ts, last_heartbeat_ts) - start_ts) / 1000
         FROM sessions
         WHERE is_idle = 0
         GROUP BY 1, process_name
         HAVING SUM(COALESCE(end_ts, last_heartbeat_ts) - start_ts) >= 1000
         ORDER BY 1 DESC, 3 DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(month, process, secs)| MonthRow { month, process, total_secs: secs })
    .collect();

    (daily, monthly)
}

fn load_korean_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/malgun.ttf") {
        fonts.font_data.insert(
            "malgun".to_owned(),
            egui::FontData::from_owned(bytes).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "malgun".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("malgun".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn fmt_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{}초", secs)
    } else if secs < 3600 {
        format!("{}분 {}초", secs / 60, secs % 60)
    } else {
        format!("{}시간 {}분", secs / 3600, (secs % 3600) / 60)
    }
}

impl eframe::App for ChronographApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCommand::TogglePause => {
                    self.paused = !self.paused;
                    let event = if self.paused { AppEvent::Pause } else { AppEvent::Resume };
                    let _ = self.rt.block_on(self.event_tx.send(event));
                    if self.paused {
                        self.current_window = None;
                        self.session_start = None;
                    }
                }
                TrayCommand::Quit => {
                    std::process::exit(0);
                }
            }
        }

        // X 버튼 → 트레이로 숨기기
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // 활성 창 변경 감지
        if self.active_rx.has_changed().unwrap_or(false) {
            let new_win = self.active_rx.borrow_and_update().clone();
            let new_proc = new_win.as_ref().map(|w| w.process_name.clone());
            let cur_proc = self.current_window.as_ref().map(|w| w.process_name.clone());

            if new_proc != cur_proc {
                if self.current_window.is_some() {
                    self.refresh_from_db();
                    // 새 프로세스가 known 목록에 없으면 자동 추가
                    if let Some(ref p) = new_proc {
                        if !self.known_processes.contains(p) {
                            self.known_processes.push(p.clone());
                            self.known_processes.sort();
                            self.visible_processes.insert(p.clone());
                        }
                    }
                }
                self.current_window = new_win;
                self.session_start = self.current_window.as_ref().map(|_| Instant::now());
            } else if let (Some(new), Some(cur)) = (&new_win, &self.current_window) {
                if new.window_title != cur.window_title {
                    self.current_window = Some(new.clone());
                }
            }
        }

        if self.last_db_refresh.elapsed().as_secs() >= 30 {
            self.refresh_from_db();
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Stats,   "📊 오늘");
                ui.selectable_value(&mut self.tab, Tab::History, "📅 기록");
                ui.selectable_value(&mut self.tab, Tab::Manage,  "⚙ 앱 관리");
            });
            ui.separator();

            match self.tab {
                Tab::Stats   => self.show_stats(ui),
                Tab::History => self.show_history(ui),
                Tab::Manage  => self.show_manage(ui),
            }
        });

        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    }

}

impl ChronographApp {
    fn show_stats(&mut self, ui: &mut egui::Ui) {
        if self.paused {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("⏸ 동작 중지됨 — 트레이 우클릭 → 동작 재개")
                        .color(egui::Color32::from_rgb(255, 180, 50))
                        .strong(),
                );
            });
            ui.separator();
        }

        let today = chrono::Local::now().format("%Y년 %m월 %d일").to_string();
        ui.label(RichText::new(format!("오늘({}) 사용 현황", today)).strong());

        if let Some(win) = &self.current_window {
            ui.horizontal(|ui| {
                ui.label(RichText::new("현재: ").weak());
                ui.label(
                    RichText::new(&win.window_title)
                        .color(egui::Color32::from_rgb(150, 200, 255))
                        .italics(),
                );
            });
        }
        ui.add_space(4.0);

        let totals = self.display_totals();

        if totals.is_empty() {
            ui.label("(기록된 데이터가 없습니다. 앱을 사용하면 자동으로 나타납니다.)");
        } else {
            let grand_total: i64 = totals.iter().map(|(_, s)| s).sum();

            ScrollArea::vertical()
                .max_height(ui.available_height() - 48.0)
                .show(ui, |ui| {
                    for (process, secs) in &totals {
                        let is_active = self
                            .current_window
                            .as_ref()
                            .map(|w| &w.process_name == process)
                            .unwrap_or(false);
                        let pct = if grand_total > 0 { *secs as f32 / grand_total as f32 } else { 0.0 };

                        ui.horizontal(|ui| {
                            let name = if is_active {
                                RichText::new(format!("▶ {}", process))
                                    .strong()
                                    .color(egui::Color32::from_rgb(80, 200, 120))
                            } else {
                                RichText::new(process.as_str()).monospace()
                            };
                            ui.label(name);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(fmt_duration(*secs)).strong());
                            });
                        });

                        ui.add(
                            egui::ProgressBar::new(pct)
                                .desired_width(ui.available_width())
                                .desired_height(10.0),
                        );
                        ui.add_space(6.0);
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("합계").strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(fmt_duration(grand_total)).strong());
                        });
                    });
                });
        }

        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.separator();
            ui.horizontal(|ui| {
                let on = autostart::is_enabled();
                if ui.button(if on { "자동시작: 켜짐" } else { "자동시작: 꺼짐" }).clicked() {
                    if on { let _ = autostart::disable(); } else { let _ = autostart::enable(); }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("새로고침").clicked() { self.refresh_from_db(); }
                });
            });
        });
    }

    fn show_history(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.history_mode, HistoryMode::Daily, "일별");
            ui.selectable_value(&mut self.history_mode, HistoryMode::Monthly, "월별");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("새로고침").clicked() { self.load_history(); }
            });
        });
        ui.separator();

        if !self.history_loaded {
            self.load_history();
        }

        match self.history_mode {
            HistoryMode::Daily => {
                if self.daily_rows.is_empty() {
                    ui.label("데이터가 없습니다.");
                    return;
                }
                let rows = self.daily_rows.clone();
                let mut cur_date = String::new();
                ScrollArea::vertical().show(ui, |ui| {
                    for row in rows.iter() {
                        if row.date != cur_date {
                            if !cur_date.is_empty() { ui.add_space(8.0); }
                            ui.label(RichText::new(&row.date).strong());
                            ui.separator();
                            cur_date = row.date.clone();
                        }
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&row.process).monospace());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(fmt_duration(row.total_secs));
                            });
                        });
                    }
                });
            }
            HistoryMode::Monthly => {
                if self.monthly_rows.is_empty() {
                    ui.label("데이터가 없습니다.");
                    return;
                }
                let rows = self.monthly_rows.clone();
                let mut cur_month = String::new();
                ScrollArea::vertical().show(ui, |ui| {
                    for row in rows.iter() {
                        if row.month != cur_month {
                            if !cur_month.is_empty() { ui.add_space(8.0); }
                            ui.label(RichText::new(&row.month).strong());
                            ui.separator();
                            cur_month = row.month.clone();
                        }
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&row.process).monospace());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(fmt_duration(row.total_secs));
                            });
                        });
                    }
                });
            }
        }
    }

    fn show_manage(&mut self, ui: &mut egui::Ui) {
        if !self.manage_loaded {
            self.load_known_processes();
        }

        ui.label(RichText::new("표시할 앱 선택").strong());
        ui.label(
            RichText::new("체크 해제한 앱은 통계에서 숨겨집니다. 기록은 계속 됩니다.")
                .weak()
                .small(),
        );
        ui.add_space(4.0);

        if ui.button("목록 새로고침").clicked() {
            self.load_known_processes();
        }
        ui.separator();

        if self.known_processes.is_empty() {
            ui.label("(아직 기록된 앱이 없습니다. 앱을 사용하면 자동으로 나타납니다.)");
        } else {
            let procs = self.known_processes.clone();
            ScrollArea::vertical().show(ui, |ui| {
                for proc in &procs {
                    let mut visible = self.visible_processes.contains(proc.as_str());
                    if ui.checkbox(&mut visible, proc).changed() {
                        if visible {
                            self.visible_processes.insert(proc.clone());
                        } else {
                            self.visible_processes.remove(proc.as_str());
                        }
                    }
                }
            });
        }
    }
}

pub fn run(
    pool: SqlitePool,
    rt: Handle,
    event_tx: tokio::sync::mpsc::Sender<AppEvent>,
    active_rx: watch::Receiver<Option<WindowInfo>>,
    tray_rx: mpsc::Receiver<TrayCommand>,
    ctx_slot: Arc<Mutex<Option<egui::Context>>>,
    start_hidden: bool,
) -> Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_title("Chronograph")
        .with_inner_size([540.0, 560.0])
        .with_visible(!start_hidden)
        .with_icon(crate::icon::viewport_icon());

    let native_opts = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Chronograph",
        native_opts,
        Box::new(move |cc| {
            Ok(Box::new(ChronographApp::new(
                pool, rt, event_tx, active_rx, tray_rx, ctx_slot, cc,
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
}
