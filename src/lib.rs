extern crate ncurses;

use ncurses::*;

use std::cell::{Cell, RefCell};
use std::ops::{Deref,DerefMut};
use std::rc::Rc;
use std::sync::{Once, ONCE_INIT};

static INIT: Once = ONCE_INIT;
static mut ROOT_CONTAINER: Option<*mut WindowContainer> = None;

pub struct Tcwm;
impl Tcwm {
    pub fn new() -> Result<Tcwm, CursesError> {
        let mut ret = Err(CursesError::CursesAlreadyInitialized);
        INIT.call_once(|| {
            initscr();
            if !has_colors() {
                panic!("No colors");
            }
            curs_set(ncurses::CURSOR_VISIBILITY::CURSOR_INVISIBLE);
            noecho();
            refresh();
            start_color();

            init_pair(Color::Selection.into(), COLOR_GREEN, COLOR_BLACK);
            init_pair(Color::Status.into(), COLOR_WHITE, COLOR_BLUE);
            init_pair(Color::StatusSelected.into(), COLOR_BLACK, COLOR_CYAN);
            init_pair(Color::Default.into(), COLOR_GREEN, COLOR_BLACK);

            let root = Box::new(WindowContainer::new());
            unsafe {
                ROOT_CONTAINER = Some(Box::into_raw(root));
            }
            ret = Ok(Tcwm)
        });
        ret
    }
}
impl Deref for Tcwm {
    type Target = WindowContainer;
    fn deref(&self) -> &Self::Target {
        unsafe {
            &*ROOT_CONTAINER.unwrap()
        }
    }
}
impl DerefMut for Tcwm {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            &mut *ROOT_CONTAINER.unwrap()
        }
    }
}
impl Drop for Tcwm {
    fn drop(&mut self) {
        endwin();
        unsafe {
            let root = Box::from_raw(ROOT_CONTAINER.unwrap());
            drop(root);
        }
    }
}


type ContainerRef = Rc<RefCell<WindowContainer>>;

pub struct WindowContainer {
    payload: Vec<WindowPayload>,
    direction: WindowSplitDirection,
    container_x: i32,
    container_y: i32,
    width: i32,
    height: i32,
    focus: usize,
    root: bool, // TODO: This is an ugly hack
}
#[derive(Copy, Clone, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}
enum WindowPayload {
    Window(Window),
    Container(ContainerRef),
}
impl WindowPayload {
    fn is_container(&self) -> bool {
        match *self {
            WindowPayload::Window(_) => false,
            WindowPayload::Container(_) => true,
        }
    }
    fn is_window(&self) -> bool {
        !self.is_container()
    }
    fn as_container(&self) -> ContainerRef {
        match *self {
            WindowPayload::Container(ref c) => c.clone(),
            _ => panic!("Not a container"),
        }
    }
    fn as_window_mut(&mut self) -> &mut Window {
        match *self {
            WindowPayload::Window(ref mut wr) => wr,
            _ => panic!("Not a window"),
        }
    }
}
impl WindowContainer {
    fn new() -> WindowContainer {
        WindowContainer::new_container(0, 0, None)
    }
    fn new_container(x: i32, y: i32, window: Option<Window>) -> WindowContainer {
        let is_root = window.is_none();
        let win = window.unwrap_or(Window::new());
        let width = win.xmax;
        let height = win.ymax;
        let mut root = WindowContainer {
            payload: vec![WindowPayload::Window(win)],
            direction: WindowSplitDirection::Vertical,
            container_x: x,
            container_y: y,
            width: width,
            height: height,
            focus: 0,
            root: is_root,
        };
        root.refresh_windows(false);
        root
    }
    pub fn change_focus(&mut self, direction: Direction) {
        let _ = self.change_focus_internal(direction);
        self.refresh_windows(false);
    }
    fn change_focus_internal(&mut self, direction: Direction) -> Result<(), ()> {
        if self.payload[self.focus].is_container() {
            let c = self.payload[self.focus].as_container();
            return match c.borrow_mut().change_focus_internal(direction) {
                Err(_) => {
                    self.do_focus_change(direction)
                }
                Ok(_) => Ok(())
            };
        } else {
            self.do_focus_change(direction)
        }
    }
    fn do_focus_change(&mut self, direction: Direction) -> Result<(), ()> {
        match direction {
            Direction::Left | Direction::Up => {
                if self.direction.direction_ok(direction) {
                    if self.focus == 0 {
                        Err(())
                    } else {
                        self.focus -= 1;
                        Ok(())
                    }
                } else { Err(()) }
            }
            Direction::Right | Direction::Down => {
                if self.direction.direction_ok(direction) {
                    if self.focus == self.payload.len() - 1 {
                        Err(())
                    } else {
                        self.focus += 1;
                        Ok(())
                    }
                } else { Err(()) }
            }
        }
    }
    pub fn print(&mut self, s: &str) {
        self.with_focused_container(|f| f.focused_window().print(s))
    }
    pub fn set_split_direction(&mut self, direction: WindowSplitDirection) {
        if self.payload.len() == 1 {
            self.direction = direction;
            return
        }
        if let WindowPayload::Container(ref container) = self.payload[self.focus] {
            container.borrow_mut().set_split_direction(direction);
            return;
        }
        let win = self.payload.remove(self.focus);
        match win {
            WindowPayload::Window(win) => {
                let x = win.x;
                let y = win.y;
                let mut new = WindowContainer::new_container(x, y, Some(win));
                new.direction = direction;
                self.payload.insert(self.focus, WindowPayload::Container(Rc::new(RefCell::new(new))));
            }
            _ => unreachable!()
        }
    }
    pub fn split(&mut self) {
        self.with_focused_container(|f| {
            let win = match f.direction {
                WindowSplitDirection::Vertical => f.split_vertical(),
                WindowSplitDirection::Horizontal => f.split_horizontal(),
            };
            f.focus += 1;
            f.payload.push(WindowPayload::Window(win));
        });
        self.refresh_windows(true);
    }
    fn with_focused_container<F, T>(&mut self, f: F) -> T
        where F: Fn(&mut WindowContainer) -> T
    {
        match self.focused_container() {
            Some(w) => f(&mut *w.borrow_mut()),
            None => f(self),
        }
    }
    fn focused_window(&mut self) -> &mut Window {
        self.payload[self.focus].as_window_mut()
    }
    fn focused_container(&self) -> Option<ContainerRef> {
        let ref win = self.payload[self.focus];
        match win {
            &WindowPayload::Container(ref container) => {
                let c = container.borrow();
                if c.payload[c.focus].is_container() {
                    c.focused_container()
                } else {
                    Some(container.clone())
                }
            },
            &WindowPayload::Window(_) => None
        }
    }
    fn calculate_dimensions(&self, window_count: Option<i32>) -> (i32, i32, i32) {
        // TODO: Not sure if this function makes any sense
        let window_count = window_count.unwrap_or(self.payload.len() as i32);
        let (dim, pos) = match self.direction {
            WindowSplitDirection::Horizontal => (self.height, self.container_y),
            WindowSplitDirection::Vertical => (self.width, self.container_x),
        };
        let size = dim / window_count;
        let rounding_error = dim % size;
        (pos, size, rounding_error)
    }
    fn refresh_dimensions(&mut self, dimensions: (i32, i32, i32)) {
        let (pos, size, _) = dimensions;
        for (i, window) in self.payload.iter_mut().enumerate() {
            match window {
                &mut WindowPayload::Window(ref mut w) => {
                    w.cursor.set((0, 0));
                    {
                        let (mut wpos, mut wdim) = match self.direction {
                            WindowSplitDirection::Horizontal => (&mut w.y, &mut w.ymax),
                            WindowSplitDirection::Vertical => (&mut w.x, &mut w.xmax),
                        };
                        *wdim = size;
                        *wpos = pos + (i as i32) * size;
                    }
                    WindowContainer::reresize_window(&mut *w);
                }
                _ => {}
            }
        }
    }
    fn split_vertical(&mut self) -> Window {
        let dim = self.calculate_dimensions(Some(self.payload.len() as i32 + 1));
        self.refresh_dimensions(dim);
        let (_, window_width, rounding_error) = dim;
        let new_window_x = self.container_x + (self.payload.len() as i32 * window_width);
        Window::new_window(new_window_x, self.container_y, (window_width + rounding_error, self.height), true)
    }
    fn split_horizontal(&mut self) -> Window {
        let dim = self.calculate_dimensions(Some(self.payload.len() as i32 + 1));
        self.refresh_dimensions(dim);
        let (_, window_height, rounding_error) = dim;
        let new_window_y = self.container_y + (self.payload.len() as i32 * window_height);
        Window::new_window(self.container_x, new_window_y, (self.width, window_height + rounding_error), self.container_x > 0)
    }
    fn reresize_window(w: &mut Window) {
        wclear(w.win);
        if let Some(bwin) = w.border_win {
            wresize(w.win, w.ymax - 1, w.xmax - 1);
            wresize(w.header_win, 1, w.xmax);
            mvwin(bwin, w.y, w.x);
            mvwin(w.header_win, w.y + w.ymax - 1, w.x);
            mvwin(w.win, w.y, w.x + 1);
        } else {
            wresize(w.win, w.ymax - 1, w.xmax);
            wresize(w.header_win, 1, w.xmax);
            mvwin(w.header_win, w.y + w.ymax - 1, w.x);
            mvwin(w.win, w.y, w.x);
        }
    }
    fn refresh_windows(&mut self, reprint: bool) {
        self.refresh_windows_internal(reprint, true)
    }
    fn refresh_windows_internal(&mut self, reprint: bool, in_focus_chain: bool) {
        for (i, window) in self.payload.iter_mut().enumerate() {
            match window {
                &mut WindowPayload::Window(ref mut w) => {
                    let header_color = {
                        let focused = in_focus_chain && self.focus == i;
                        let color = if focused { Color::StatusSelected } else { Color::Status };
                        COLOR_PAIR(color.into())
                    };
                    wbkgd(w.header_win, header_color);
                    if self.root {
                        w.header = format!("Window {} ({}, {}) ({}, {})", i, w.x, w.y, w.xmax, w.ymax);
                    } else {
                        w.header = format!("Container {} ({}, {}) ({}, {})", i, w.x, w.y, w.xmax, w.ymax);
                    }
                    w.print_header();
                    if let Some(bwin) = w.border_win {
                        mvwvline(bwin, 0, 0, ACS_HLINE(), 1000);
                        wrefresh(bwin);
                    }
                    if reprint {
                        w.reprint_buffer();
                    } else {
                        wrefresh(w.win);
                    }
                }
                &mut WindowPayload::Container(ref c) => {
                    let mut c = c.borrow_mut();
                    let in_focus_chain = in_focus_chain && self.focus == i;
                    c.refresh_windows_internal(reprint, in_focus_chain);
                }
            }
        }
    }
    pub fn set_header(&mut self, header: &str) {
        self.with_focused_container(|w| {
            let mut w = w.payload[w.focus].as_window_mut();
            w.header = header.to_owned();
            w.print_header();
        })
    }
    fn on_resize(&mut self) {
        let mut h = 0;
        let mut w = 0;
        getmaxyx(stdscr, &mut h, &mut w);
        self.height = h;
        self.width = w;
        for pl in self.payload.iter_mut() {
            if pl.is_window() {
                let mut win = pl.as_window_mut();
                win.xmax = w;
                win.ymax = h;
                WindowContainer::reresize_window(win);
            }
        }
        self.refresh_windows(false);
    }
    pub fn wait_for_key(&mut self) -> i32 {
        let ret = self.with_focused_container(|mut f| {
            let ret = ncurses::wgetch(f.focused_window().win);
            if ret == ncurses::KEY_RESIZE {
                unsafe {
                    let ref mut rc = *ROOT_CONTAINER.unwrap();
                    rc.on_resize();
                }
            }
            ret
        });
        ret
    }
}

pub enum CursesError {
    CursesAlreadyInitialized,
}

#[derive(Copy, Clone, PartialEq)]
pub enum WindowSplitDirection {
    Horizontal,
    Vertical,
}
impl WindowSplitDirection {
    fn direction_ok(&self, dir: Direction) -> bool {
        match *self {
            WindowSplitDirection::Vertical => {
                dir == Direction::Left
                || dir == Direction::Right
            }
            WindowSplitDirection::Horizontal => {
                dir == Direction::Up
                || dir == Direction::Down
            }
        }
    }
}

enum Color {
    Default = 1,
    Selection = 2,
    Status = 3,
    StatusSelected = 4,
}
impl Into<i16> for Color {
    fn into(self) -> i16 {
        self as i16
    }
}

struct Window {
    win: WINDOW,
    border_win: Option<WINDOW>,
    header_win: WINDOW,
    x: i32,
    y: i32,
    xmax: i32,
    ymax: i32,
    cursor: Cell<(i32, i32)>,
    lines: Vec<String>,
    header: String,
}
impl Window {
    fn new_window(x: i32, y: i32, dimensions: (i32, i32), border: bool) -> Window {
        let (xmax, ymax) = dimensions;
        let bwin = if border { Some(newwin(ymax, 1, y, x)) } else { None };
        let win = if border { newwin(ymax-1, xmax, y, x+1) } else { newwin(ymax-1, xmax, y, x) };
        let hwin = newwin(1, xmax, y + ymax - 1, x);
        wbkgd(hwin, COLOR_PAIR(Color::StatusSelected.into()));
        Window {
            win: win,
            border_win: bwin,
            header_win: hwin,
            x: x,
            y: y,
            xmax: xmax,
            ymax: ymax,
            cursor: Cell::new((0, 0)),
            lines: vec![],
            header: "New window".into(),
        }
    }
    fn new() -> Window {
        let mut xmax = 0;
        let mut ymax = 0;
        getmaxyx(stdscr, &mut ymax, &mut xmax);
        Window::new_window(0, 0, (xmax, ymax), false)
    }
    fn reprint_buffer(&mut self) {
        for line in self.lines.iter() {
            self.print_internal(line);
        }
        wrefresh(self.win);
    }
    fn print_header(&mut self) {
        let margin = if self.border_win.is_some() { 2 } else { 1 };
        mvwprintw(self.header_win, 0, margin, &self.header[..]);
        wrefresh(self.header_win);
    }
    fn print_internal(&self, s: &str) {
        let (x, mut y) = self.cursor.get();
        if y >= self.ymax {
            // TODO: Scroll
            return;
        }
        mvwprintw(self.win, y, x, s);
        y += (s.len() as i32 / (self.xmax - 1)) + 1;
        self.cursor.set((x, y));
    }
    pub fn print(&mut self, s: &str) {
        self.print_internal(s);
        self.lines.push(s.into());
        wrefresh(self.win);
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        delwin(self.win);
        if let Some(bwin) = self.border_win {
            delwin(bwin);
        }
    }
}
