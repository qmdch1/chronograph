use anyhow::Result;
use std::sync::{Arc, Mutex, mpsc};
use tray_icon::{
    TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

#[derive(Debug)]
pub enum TrayCommand {
    TogglePause,
    Quit,
}

/// egui Context는 창이 숨겨진 동안 ui()가 호출되지 않으므로,
/// tray 핸들러에서 직접 들고 있어야 ShowWindow가 즉시 동작함.
pub fn create(
    ctx_slot: Arc<Mutex<Option<egui::Context>>>,
) -> Result<(tray_icon::TrayIcon, mpsc::Receiver<TrayCommand>)> {
    let (tx, rx) = mpsc::channel::<TrayCommand>();

    let show_item  = MenuItem::new("열기", true, None);
    let pause_item = MenuItem::new("동작 중지", true, None);
    let quit_item  = MenuItem::new("종료", true, None);

    let show_id  = show_item.id().clone();
    let pause_id = pause_item.id().clone();
    let quit_id  = quit_item.id().clone();

    let menu = Menu::new();
    menu.append(&show_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&pause_item)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit_item)?;

    let icon = crate::icon::tray_icon()?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_menu_on_right_click(true)
        .with_tooltip("Chronograph")
        .with_icon(icon)
        .build()?;

    // 더블클릭 → ctx에 직접 show 커맨드
    let ctx_click = ctx_slot.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { .. } = event {
            show_window(&ctx_click);
        }
    }));

    // 메뉴 이벤트
    let ctx_menu = ctx_slot.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id() == &show_id {
            show_window(&ctx_menu);
        } else if event.id() == &pause_id {
            let _ = tx.send(TrayCommand::TogglePause);
        } else if event.id() == &quit_id {
            let _ = tx.send(TrayCommand::Quit);
        }
    }));

    Ok((tray, rx))
}

fn show_window(ctx_slot: &Arc<Mutex<Option<egui::Context>>>) {
    if let Some(ctx) = ctx_slot.lock().unwrap().as_ref() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }
}
