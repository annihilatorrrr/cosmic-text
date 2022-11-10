// SPDX-License-Identifier: MIT OR Apache-2.0

use cosmic_text::{
    Attrs,
    AttrsList,
    Buffer,
    Color,
    Editor,
    Family,
    FontSystem,
    Metrics,
    Style,
    SwashCache,
    Action,
    Weight
};
use orbclient::{EventOption, Renderer, Window, WindowFlag};
use std::{env, fs, thread, time::{Duration, Instant}};
use syntect::highlighting::{
    FontStyle,
    Highlighter,
    HighlightState,
    RangedHighlightIterator,
    ThemeSet,
};
use syntect::parsing::{
    ParseState,
    ScopeStack,
    SyntaxSet,
};

fn main() {
    env_logger::init();

    let (path, text) = if let Some(arg) = env::args().nth(1) {
        (
            arg.clone(),
            fs::read_to_string(&arg).expect("failed to open file")
        )
    } else {
        (
            String::new(),
            String::new()
        )
    };

    let display_scale = match orbclient::get_display_size() {
        Ok((w, h)) => {
            log::info!("Display size: {}, {}", w, h);
            (h as i32 / 1600) + 1
        }
        Err(err) => {
            log::warn!("Failed to get display size: {}", err);
            1
        }
    };

    let mut window = Window::new_flags(
        -1,
        -1,
        1024 * display_scale as u32,
        768 * display_scale as u32,
        &format!("COSMIC Text - {}", path),
        &[WindowFlag::Resizable],
    )
    .unwrap();

    let mut font_system = FontSystem::new();

    let font_sizes = [
        Metrics::new(10, 14).scale(display_scale), // Caption
        Metrics::new(14, 20).scale(display_scale), // Body
        Metrics::new(20, 28).scale(display_scale), // Title 4
        Metrics::new(24, 32).scale(display_scale), // Title 3
        Metrics::new(28, 36).scale(display_scale), // Title 2
        Metrics::new(32, 44).scale(display_scale), // Title 1
    ];
    let font_size_default = 1; // Body
    let mut font_size_i = font_size_default;

    let line_x = 8 * display_scale;
    let mut editor = Editor::new(Buffer::new(
        &mut font_system,
        font_sizes[font_size_i]
    ));

    editor.buffer.set_size(
        &mut font_system,
        window.width() as i32 - line_x * 2,
        window.height() as i32
    );

    let attrs = Attrs::new()
        .monospaced(true)
        .family(Family::Monospace);
    editor.buffer.set_text(&mut font_system, &text, attrs);

    let mut bg_color = orbclient::Color::rgb(0x00, 0x00, 0x00);
    let mut font_color = Color::rgb(0xFF, 0xFF, 0xFF);

    let now = Instant::now();

    //TODO: store newlines in buffer
    let ps = SyntaxSet::load_defaults_nonewlines();
    let ts = ThemeSet::load_defaults();
    let theme = &ts.themes["base16-eighties.dark"];
    let highlighter = Highlighter::new(theme);

    if let Some(background) = theme.settings.background {
        bg_color = orbclient::Color::rgba(
            background.r,
            background.g,
            background.b,
            background.a,
        );
    }

    if let Some(foreground) = theme.settings.foreground {
        font_color = Color::rgba(
            foreground.r,
            foreground.g,
            foreground.b,
            foreground.a,
        );
    }

    let syntax = match ps.find_syntax_for_file(&path) {
        Ok(Some(some)) => some,
        Ok(None) => {
            log::warn!("no syntax found for {:?}", path);
            ps.find_syntax_plain_text()
        }
        Err(err) => {
            log::warn!("failed to determine syntax for {:?}: {:?}", path, err);
            ps.find_syntax_plain_text()
        }
    };

    log::info!("using syntax {:?}, loaded in {:?}", syntax.name, now.elapsed());

    let mut swash_cache = SwashCache::new();

    let mut syntax_cache = Vec::<(ParseState, HighlightState)>::new();

    let mut ctrl_pressed = false;
    let mut mouse_x = -1;
    let mut mouse_y = -1;
    let mut mouse_left = false;
    let mut rehighlight = true;
    loop {
        if rehighlight {
            let now = Instant::now();

            for line_i in 0..editor.buffer.lines.len() {
                let line = &mut editor.buffer.lines[line_i];
                if ! line.is_reset() && line_i < syntax_cache.len() {
                    continue;
                }

                let (mut parse_state, mut highlight_state) = if line_i > 0 && line_i <= syntax_cache.len() {
                    syntax_cache[line_i - 1].clone()
                } else {
                    (
                        ParseState::new(syntax),
                        HighlightState::new(&highlighter, ScopeStack::new())
                    )
                };

                let ops = parse_state.parse_line(line.text(), &ps).unwrap();
                let ranges = RangedHighlightIterator::new(
                    &mut highlight_state,
                    &ops,
                    line.text(),
                    &highlighter,
                );

                let mut attrs_list = AttrsList::new(attrs);
                for (style, _, range) in ranges {
                    attrs_list.add_span(
                        range,
                        attrs
                            .color(Color::rgba(
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                                style.foreground.a,
                            ))
                            //TODO: background
                            .style(if style.font_style.contains(FontStyle::ITALIC) {
                                Style::Italic
                            } else {
                                Style::Normal
                            })
                            .weight(if style.font_style.contains(FontStyle::BOLD) {
                                Weight::BOLD
                            } else {
                                Weight::NORMAL
                            })
                            //TODO: underline
                    );
                }

                // Update line attributes. This operation only resets if the line changes
                line.set_attrs_list(attrs_list);
                line.set_wrap_simple(true);

                //TODO: efficiently do syntax highlighting without having to shape whole buffer
                line.shape(&mut font_system);

                let cache_item = (parse_state.clone(), highlight_state.clone());
                if line_i < syntax_cache.len() {
                    if syntax_cache[line_i] != cache_item {
                        syntax_cache[line_i] = cache_item;
                        if line_i + 1 < editor.buffer.lines.len() {
                            editor.buffer.lines[line_i + 1].reset();
                        }
                    }
                } else {
                    syntax_cache.push(cache_item);
                }
            }

            editor.buffer.redraw = true;
            rehighlight = false;

            log::info!("Syntax highlighted in {:?}", now.elapsed());
        }

        editor.shape_as_needed(&mut font_system);
        if editor.buffer.redraw {
            let instant = Instant::now();

            window.set(bg_color);

            editor.draw(&mut font_system, &mut swash_cache, font_color, |x, y, w, h, color| {
                window.rect(line_x + x, y, w, h, orbclient::Color { data: color.0 })
            });

            // Draw scrollbar
            {
                let mut start_line_opt = None;
                let mut end_line = 0;
                for run in editor.buffer.layout_runs() {
                    end_line = run.line_i;
                    if start_line_opt == None {
                        start_line_opt = Some(end_line);
                    }
                }

                let start_line = start_line_opt.unwrap_or(end_line);
                let lines = editor.buffer.lines.len();
                let start_y = (start_line * window.height() as usize) / lines;
                let end_y = (end_line * window.height() as usize) / lines;
                if end_y > start_y {
                    window.rect(
                        window.width() as i32 - line_x as i32,
                        start_y as i32,
                        line_x as u32,
                        (end_y - start_y) as u32,
                        orbclient::Color::rgba(0xFF, 0xFF, 0xFF, 0x40),
                    );
                }
            }

            window.sync();

            editor.buffer.redraw = false;

            let duration = instant.elapsed();
            log::debug!("redraw: {:?}", duration);
        }

        let mut found_event = false;
        let mut force_drag = true;
        let mut window_async = false;
        for event in window.events() {
            found_event = true;
            match event.to_option() {
                EventOption::Key(event) => match event.scancode {
                    orbclient::K_CTRL => ctrl_pressed = event.pressed,
                    orbclient::K_LEFT if event.pressed => editor.action(&mut font_system, Action::Left),
                    orbclient::K_RIGHT if event.pressed => editor.action(&mut font_system, Action::Right),
                    orbclient::K_UP if event.pressed => editor.action(&mut font_system, Action::Up),
                    orbclient::K_DOWN if event.pressed => editor.action(&mut font_system, Action::Down),
                    orbclient::K_HOME if event.pressed => editor.action(&mut font_system, Action::Home),
                    orbclient::K_END if event.pressed => editor.action(&mut font_system, Action::End),
                    orbclient::K_PGUP if event.pressed => editor.action(&mut font_system, Action::PageUp),
                    orbclient::K_PGDN if event.pressed => editor.action(&mut font_system, Action::PageDown),
                    orbclient::K_ENTER if event.pressed => {
                        editor.action(&mut font_system, Action::Enter);
                        rehighlight = true;
                    },
                    orbclient::K_BKSP if event.pressed => {
                        editor.action(&mut font_system, Action::Backspace);
                        rehighlight = true;
                    },
                    orbclient::K_DEL if event.pressed => {
                        editor.action(&mut font_system, Action::Delete);
                        rehighlight = true;
                    },
                    orbclient::K_0 if event.pressed && ctrl_pressed => {
                        font_size_i = font_size_default;
                        editor.buffer.set_metrics(&mut font_system, font_sizes[font_size_i]);
                    }
                    orbclient::K_MINUS if event.pressed && ctrl_pressed => {
                        if font_size_i > 0 {
                            font_size_i -= 1;
                            editor.buffer.set_metrics(&mut font_system, font_sizes[font_size_i]);
                        }
                    }
                    orbclient::K_EQUALS if event.pressed && ctrl_pressed => {
                        if font_size_i + 1 < font_sizes.len() {
                            font_size_i += 1;
                            editor.buffer.set_metrics(&mut font_system, font_sizes[font_size_i]);
                        }
                    }
                    _ => (),
                },
                EventOption::TextInput(event) if !ctrl_pressed => {
                    editor.action(&mut font_system, Action::Insert(event.character));
                    rehighlight = true;
                }
                EventOption::Mouse(event) => {
                    mouse_x = event.x;
                    mouse_y = event.y;
                    if mouse_left {
                        editor.action(&mut font_system, Action::Drag {
                            x: mouse_x - line_x,
                            y: mouse_y,
                        });

                        if mouse_y <= 5 {
                            editor.action(&mut font_system, Action::Scroll { lines: -3 });
                            window_async = true;
                        } else if mouse_y + 5 >= window.height() as i32 {
                            editor.action(&mut font_system, Action::Scroll { lines: 3 });
                            window_async = true;
                        }

                        force_drag = false;
                    }
                }
                EventOption::Button(event) => {
                    if event.left != mouse_left {
                        mouse_left = event.left;
                        if mouse_left {
                            editor.action(&mut font_system, Action::Click {
                                x: mouse_x - line_x,
                                y: mouse_y,
                            });
                        }
                        force_drag = false;
                    }
                }
                EventOption::Resize(event) => {
                    editor.buffer.set_size(&mut font_system, event.width as i32 - line_x * 2, event.height as i32);
                }
                EventOption::Scroll(event) => {
                    editor.action(&mut font_system, Action::Scroll {
                        lines: -event.y * 3,
                    });
                }
                EventOption::Quit(_) => return,
                _ => (),
            }
        }

        if mouse_left && force_drag {
            editor.action(&mut font_system, Action::Drag {
                x: mouse_x - line_x,
                y: mouse_y,
            });

            if mouse_y <= 5 {
                editor.action(&mut font_system, Action::Scroll { lines: -3 });
                window_async = true;
            } else if mouse_y + 5 >= window.height() as i32 {
                editor.action(&mut font_system, Action::Scroll { lines: 3 });
                window_async = true;
            }
        }

        if window_async != window.is_async() {
            window.set_async(window_async);
        }

        if window_async && ! found_event {
            // In async mode and no event found, sleep
            thread::sleep(Duration::from_millis(5));
        }
    }
}
