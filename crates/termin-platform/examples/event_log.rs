//! Example testing the Termin macOS platform layer.
//!
//! Opens two windows, clears each with a distinct color via raw Metal,
//! and logs all platform events to stdout.

use termin_core::{Key, NamedKey, PaneId, Rect, TabId, WindowId, WorkspaceId};
use termin_platform::{
    ActionId, AppHandler, MacPlatform, MenuItemSpec, MenuSpec, Platform, PlatformCx, PlatformEvent,
    RenderTarget, SystemNotification, WindowOpts,
};

#[cfg(target_os = "macos")]
use objc2::runtime::ProtocolObject;
#[cfg(target_os = "macos")]
use objc2_metal::{
    MTLClearColor, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLDevice, MTLLoadAction,
    MTLRenderPassDescriptor, MTLStoreAction,
};
#[cfg(target_os = "macos")]
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};

struct EventLogApp {
    win1: Option<WindowId>,
    win2: Option<WindowId>,
    #[cfg(target_os = "macos")]
    command_queue: Option<objc2::rc::Retained<ProtocolObject<dyn MTLCommandQueue>>>,
}

impl EventLogApp {
    fn new() -> Self {
        Self {
            win1: None,
            win2: None,
            #[cfg(target_os = "macos")]
            command_queue: None,
        }
    }
}

impl AppHandler for EventLogApp {
    fn init(&mut self, cx: &mut PlatformCx) {
        println!("=== EventLogApp::init ===");

        // Setup Main Menu
        let app_menu = MenuItemSpec {
            title: "Termin".into(),
            action: None,
            key_equivalent: None,
            submenu: Some(MenuSpec {
                title: "Termin".into(),
                items: vec![
                    MenuItemSpec {
                        title: "About Termin".into(),
                        action: Some(ActionId(1)),
                        key_equivalent: None,
                        submenu: None,
                    },
                    MenuItemSpec {
                        title: "Quit Termin".into(),
                        action: Some(ActionId(2)),
                        key_equivalent: Some("q".into()),
                        submenu: None,
                    },
                ],
            }),
        };

        let shell_menu = MenuItemSpec {
            title: "Shell".into(),
            action: None,
            key_equivalent: None,
            submenu: Some(MenuSpec {
                title: "Shell".into(),
                items: vec![
                    MenuItemSpec {
                        title: "Toggle Fullscreen".into(),
                        action: Some(ActionId(3)),
                        key_equivalent: Some("f".into()),
                        submenu: None,
                    },
                    MenuItemSpec {
                        title: "Post Notification".into(),
                        action: Some(ActionId(4)),
                        key_equivalent: Some("n".into()),
                        submenu: None,
                    },
                ],
            }),
        };

        let root_menu = MenuSpec {
            title: "".into(),
            items: vec![app_menu, shell_menu],
        };
        cx.set_menu(&root_menu);

        // Open Window 1: Slate Blue
        let w1 = cx.create_window(&WindowOpts {
            title: Some("Termin Window 1 (Slate Blue)".into()),
            frame: Some(Rect {
                x: 100.0,
                y: 100.0,
                w: 700.0,
                h: 500.0,
            }),
            restore_id: None,
            opacity: Some(1.0),
            blur: Some(false),
        });
        cx.set_drag_regions(
            w1,
            vec![Rect {
                x: 0.0,
                y: 0.0,
                w: 700.0,
                h: 38.0,
            }],
        );
        self.win1 = Some(w1);

        // Open Window 2: Crimson
        let w2 = cx.create_window(&WindowOpts {
            title: Some("Termin Window 2 (Crimson)".into()),
            frame: Some(Rect {
                x: 840.0,
                y: 100.0,
                w: 700.0,
                h: 500.0,
            }),
            restore_id: None,
            opacity: Some(1.0),
            blur: Some(false),
        });
        cx.set_drag_regions(
            w2,
            vec![Rect {
                x: 0.0,
                y: 0.0,
                w: 700.0,
                h: 38.0,
            }],
        );
        self.win2 = Some(w2);

        // Request initial frames
        cx.request_frame(w1);
        cx.request_frame(w2);

        println!("Opened Window 1: {:?}, Window 2: {:?}", w1, w2);
        println!("Controls:");
        println!("  'f' - Toggle fullscreen");
        println!("  'c' - Copy test string to clipboard");
        println!("  'v' - Read clipboard to stdout");
        println!("  'n' - Send test notification");
        println!("  'b' - Toggle dock badge");
        println!("  'q' - Close active window");
    }

    fn event(&mut self, cx: &mut PlatformCx, win: WindowId, ev: PlatformEvent) {
        println!("[Event][Win {:?}] {:?}", win, ev);

        match &ev {
            PlatformEvent::CloseRequested => {
                println!("Close requested for window {:?}, closing...", win);
                cx.close_window(win);
                if self.win1 == Some(win) {
                    self.win1 = None;
                } else if self.win2 == Some(win) {
                    self.win2 = None;
                }
            }
            PlatformEvent::Resized { size, .. } => {
                cx.set_drag_regions(
                    win,
                    vec![Rect {
                        x: 0.0,
                        y: 0.0,
                        w: size.w,
                        h: 38.0,
                    }],
                );
                cx.request_frame(win);
            }
            PlatformEvent::Key(key_ev) if key_ev.pressed => match key_ev.key {
                Key::Char('f') => {
                    println!("Toggling fullscreen on {:?}", win);
                    cx.toggle_fullscreen(win);
                }
                Key::Char('c') => {
                    let text = format!("Termin copied text from window {:?}", win);
                    println!("Writing to clipboard: {}", text);
                    cx.clipboard_set(&text);
                }
                Key::Char('v') => {
                    println!("Clipboard content: {:?}", cx.clipboard_get());
                }
                Key::Char('n') => {
                    println!("Posting notification...");
                    cx.notify(&SystemNotification {
                        title: "Termin Event Log".into(),
                        body: format!("Triggered from window {:?}", win),
                        window: win,
                        workspace: WorkspaceId(1),
                        pane: PaneId(1),
                        tab: TabId(1),
                    });
                }
                Key::Char('b') => {
                    println!("Setting dock badge to 1");
                    cx.set_badge(1);
                }
                Key::Char('q') | Key::Named(NamedKey::Escape) => {
                    println!("Closing window {:?}", win);
                    cx.close_window(win);
                    if self.win1 == Some(win) {
                        self.win1 = None;
                    } else if self.win2 == Some(win) {
                        self.win2 = None;
                    }
                }
                _ => {}
            },
            PlatformEvent::FileDrop(paths) => {
                println!("Dropped files on window {:?}: {:?}", win, paths);
            }
            PlatformEvent::ImeCompose { text, cursor } => {
                println!(
                    "IME composing on window {:?}: text='{}', cursor={:?}",
                    win, text, cursor
                );
            }
            PlatformEvent::ImeCommit(text) => {
                println!("IME commit on window {:?}: text='{}'", win, text);
            }
            _ => {}
        }
    }

    fn frame(&mut self, _cx: &mut PlatformCx, win: WindowId, target: &mut dyn RenderTarget) {
        #[cfg(target_os = "macos")]
        {
            let layer_ptr = target.metal_layer();
            if layer_ptr.is_null() {
                return;
            }

            // SAFETY: layer_ptr is guaranteed to be a valid CAMetalLayer pointer on macOS.
            let layer: &CAMetalLayer = unsafe { &*(layer_ptr as *const CAMetalLayer) };

            if self.command_queue.is_none() {
                // SAFETY: Obtaining device associated with CAMetalLayer.
                if let Some(dev) = unsafe { layer.device() } {
                    self.command_queue = dev.newCommandQueue();
                }
            }

            let Some(queue) = self.command_queue.as_ref() else {
                return;
            };

            // Pick distinct clear colors:
            // Window 1: Slate Blue (0.12, 0.16, 0.28)
            // Window 2: Crimson    (0.35, 0.08, 0.15)
            let clear_color = if self.win1 == Some(win) {
                MTLClearColor {
                    red: 0.12,
                    green: 0.16,
                    blue: 0.28,
                    alpha: 1.0,
                }
            } else {
                MTLClearColor {
                    red: 0.35,
                    green: 0.08,
                    blue: 0.15,
                    alpha: 1.0,
                }
            };

            // SAFETY: Obtaining drawable and encoding clear render pass with Metal.
            unsafe {
                let Some(drawable) = layer.nextDrawable() else {
                    return;
                };

                let pass_desc = MTLRenderPassDescriptor::renderPassDescriptor();
                let color_attachment = pass_desc.colorAttachments().objectAtIndexedSubscript(0);
                color_attachment.setTexture(Some(&drawable.texture()));
                color_attachment.setLoadAction(MTLLoadAction::Clear);
                color_attachment.setClearColor(clear_color);
                color_attachment.setStoreAction(MTLStoreAction::Store);

                if let Some(command_buffer) = queue.commandBuffer() {
                    if let Some(encoder) =
                        command_buffer.renderCommandEncoderWithDescriptor(&pass_desc)
                    {
                        encoder.endEncoding();
                    }
                    command_buffer.presentDrawable(ProtocolObject::from_ref(&*drawable));
                    command_buffer.commit();
                }
            }
        }
    }

    fn wake(&mut self, _cx: &mut PlatformCx) {
        println!("[Event] Wake received on main thread");
    }

    fn menu_action(&mut self, cx: &mut PlatformCx, action: ActionId) {
        println!("[Event] Menu Action triggered: {:?}", action);
        match action {
            ActionId(1) => {
                println!("About Termin: Native macOS Terminal in Rust");
            }
            ActionId(2) => {
                println!("Quit Termin requested via menu");
                if let Some(w1) = self.win1 {
                    cx.close_window(w1);
                }
                if let Some(w2) = self.win2 {
                    cx.close_window(w2);
                }
            }
            ActionId(3) => {
                if let Some(win) = self.win1 {
                    cx.toggle_fullscreen(win);
                }
            }
            ActionId(4) => {
                if let Some(win) = self.win1.or(self.win2) {
                    cx.notify(&SystemNotification {
                        title: "Termin Menu Notification".into(),
                        body: "Notification from Shell menu".into(),
                        window: win,
                        workspace: WorkspaceId(1),
                        pane: PaneId(1),
                        tab: TabId(1),
                    });
                }
            }
            _ => {}
        }
    }

    fn should_terminate(&mut self, _cx: &mut PlatformCx) -> bool {
        println!("should_terminate called");
        true
    }
}

fn main() {
    println!("Starting Termin event_log example...");
    MacPlatform::run(EventLogApp::new());
}
