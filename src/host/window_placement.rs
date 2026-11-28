use std::{
    collections::HashMap,
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui::{
    App, AppContext, Bounds, DisplayId, Entity, Global, Pixels, Task, Window, WindowBounds,
    WindowId, point, px, size,
};
use jcode_desktop_api::LaunchMode;
use jcode_desktop_model::window_placement::{DisplayArea, Placement, Presentation, Rect};

const DEBOUNCE: Duration = Duration::from_millis(250);

struct Placements {
    dir: PathBuf,
    windows: HashMap<WindowId, Entity<Observer>>,
}

impl Global for Placements {}

impl Placements {
    fn path(&self, mode: LaunchMode) -> PathBuf {
        self.dir.join(match mode {
            LaunchMode::Workspace => "window-placement-workspace.json",
            LaunchMode::NoSidebar => "window-placement-no-sidebar.json",
            LaunchMode::SinglePanel => "window-placement-single-panel.json",
        })
    }
}

pub fn install(dir: PathBuf, cx: &mut App) {
    cx.set_global(Placements {
        dir,
        windows: HashMap::new(),
    });
    cx.on_window_closed(|cx, id| {
        if let Some(observer) = cx.global_mut::<Placements>().windows.remove(&id) {
            // GPUI has already removed the window. Flush its last observed bounds
            // for close paths owned by the UI, including shared single-panel windows.
            observer.update(cx, |observer, _| observer.save());
        }
    })
    .detach();
    cx.on_app_quit(|cx| {
        let active = cx.active_window().map(|window| window.window_id());
        let mut newest = HashMap::new();
        for (id, observer) in &cx.global::<Placements>().windows {
            let observer = observer.read(cx);
            let priority = (Some(*id) == active, observer.changed);
            let choice = newest.entry(&observer.path).or_insert((priority, *id));
            if priority > choice.0 {
                *choice = (priority, *id);
            }
        }
        let latest: Vec<_> = newest.into_values().map(|(_, id)| id).collect();
        for handle in cx.windows() {
            if latest.contains(&handle.window_id()) {
                let _ = handle.update(cx, |_, window, cx| flush(window, cx));
            }
        }
        std::future::ready(())
    })
    .detach();
}

pub fn restore(mode: LaunchMode, cx: &App) -> Option<(DisplayId, WindowBounds)> {
    let saved = Placement::load(&cx.try_global::<Placements>()?.path(mode))?;
    let displays = cx.displays();
    let areas: Vec<_> = displays
        .iter()
        .map(|display| DisplayArea {
            uuid: display.uuid().ok().map(|uuid| uuid.to_string()),
            visible: to_rect(display.visible_bounds()),
        })
        .collect();
    let (index, placement) = saved.restore(&areas)?;
    let rect = placement.rect;
    let bounds = Bounds::new(
        point(px(rect.x), px(rect.y)),
        size(px(rect.width), px(rect.height)),
    );
    let bounds = match placement.presentation {
        Presentation::Windowed => WindowBounds::Windowed(bounds),
        Presentation::Maximized => WindowBounds::Maximized(bounds),
        Presentation::Fullscreen => WindowBounds::Fullscreen(bounds),
    };
    Some((displays[index].id(), bounds))
}

pub fn observe(mode: LaunchMode, bounds: WindowBounds, window: &mut Window, cx: &mut App) {
    let Some(placements) = cx.try_global::<Placements>() else {
        return;
    };
    let path = placements.path(mode);
    let initial = placement(bounds, display_uuid(window, cx));
    let observer = cx.new(|cx| {
        // Observe from a host-owned entity, not HostFallback, whose root is
        // dropped as soon as the UI plugin activates.
        cx.observe_window_bounds(window, |observer: &mut Observer, window, cx| {
            observer.capture(window, cx);
            observer.pending = Some(window.spawn(cx, async move |cx| {
                cx.background_executor().timer(DEBOUNCE).await;
                let _ = cx.update(flush);
            }));
        })
        .detach();
        Observer {
            path,
            latest: initial.clone(),
            last_windowed: initial,
            changed: Instant::now(),
            pending: None,
        }
    });
    cx.global_mut::<Placements>()
        .windows
        .insert(window.window_handle().window_id(), observer);
}

pub fn flush(window: &mut Window, cx: &mut App) {
    let Some(observer) = cx
        .try_global::<Placements>()
        .and_then(|state| state.windows.get(&window.window_handle().window_id()))
        .cloned()
    else {
        return;
    };
    observer.update(cx, |observer, cx| {
        observer.capture(window, cx);
        observer.save();
    });
}

struct Observer {
    path: PathBuf,
    latest: Placement,
    last_windowed: Placement,
    changed: Instant,
    pending: Option<Task<()>>,
}

impl Observer {
    fn capture(&mut self, window: &Window, cx: &App) {
        let mut next = placement(window.window_bounds(), display_uuid(window, cx));
        // AppKit's GPUI backend reports a zoomed window's expanded frame as
        // Windowed. Preserve the last normal frame so unzoom restores it.
        if matches!(next.presentation, Presentation::Windowed) && window.is_maximized() {
            next.presentation = Presentation::Maximized;
            if next.display_uuid == self.last_windowed.display_uuid {
                next.rect = self.last_windowed.rect;
            }
        }
        if next != self.latest {
            self.latest = next;
            self.changed = Instant::now();
        }
    }

    fn save(&mut self) {
        self.pending.take();
        if let Err(error) = self.latest.save(&self.path) {
            eprintln!("failed to save desktop window placement: {error:#}");
        }
        if self.latest.presentation == Presentation::Windowed {
            self.last_windowed = self.latest.clone();
        }
    }
}

fn placement(bounds: WindowBounds, display_uuid: Option<String>) -> Placement {
    Placement {
        rect: to_rect(bounds.get_bounds()),
        presentation: match bounds {
            WindowBounds::Windowed(_) => Presentation::Windowed,
            WindowBounds::Maximized(_) => Presentation::Maximized,
            WindowBounds::Fullscreen(_) => Presentation::Fullscreen,
        },
        display_uuid,
    }
}

fn display_uuid(window: &Window, cx: &App) -> Option<String> {
    window.display(cx)?.uuid().ok().map(|uuid| uuid.to_string())
}

fn to_rect(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        x: bounds.origin.x.as_f32(),
        y: bounds.origin.y.as_f32(),
        width: bounds.size.width.as_f32(),
        height: bounds.size.height.as_f32(),
    }
}
