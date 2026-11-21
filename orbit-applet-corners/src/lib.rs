// SPDX-License-Identifier: GPL-3.0-only

mod app;
mod config;
mod localize;
mod number_box;

pub fn run() -> cosmic::iced::Result {
    localize::localize();

    cosmic::applet::run::<app::Corners>(())
}
