/*
 *  Me TV — It's TV for me computer.
 *
 *  A GTK+/GStreamer client for watching and recording DVB.
 *
 *  Copyright © 2017, 2018  Russel Winder
 *
 *  This program is free software: you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation, either version 3 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program. If not, see <http://www.gnu.org/licenses/>.
 */

use std::fs;
use std::sync::mpsc::{Receiver, Sender};
use std::{thread, time};

use std::os::unix::fs::FileTypeExt;

use inotify_daemon::Message as IN_Message;

/// A struct to represent the identity of a specific frontend currently
/// available on the system.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrontendId {
    pub adapter: u16,
    pub frontend: u16,
}

///  A struct to represent a tuning of a frontend.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuningId {
    pub frontend: FrontendId,
    pub channel: String,
}

/// An enumeration of all the message types that  can be sent by
/// the frontend manager.
pub enum Message {
    AdapterDisappeared{id: u16},
    FrontendAppeared{fei: FrontendId},
}

/// The path in the filesystem to the DVB related special files.
pub static DVB_BASE_PATH: &str = "/dev/dvb";

/// Return the path to the adapter director for a given adapter.
pub fn adapter_path(id: u16) -> String { DVB_BASE_PATH.to_owned() + "/adapter" + &id.to_string() }

/// Return the path to the special file for a given frontend.
pub fn frontend_path(fei: &FrontendId) -> String { adapter_path(fei.adapter) + "/frontend" + &fei.frontend.to_string() }

/// Return the path to the special file of the demux for a given frontend.
pub fn demux_path(fei: &FrontendId) -> String { adapter_path(fei.adapter) + "/demux" + &fei.frontend.to_string() }

/// Return the path to the special file of the data for a given frontend.
pub fn dvr_path(fei: &FrontendId) -> String { adapter_path(fei.adapter) + "/dvr" + &fei.frontend.to_string() }

/// Process a newly present adapter to inform the control window of all the frontends
/// newly accessible.
fn add_frontends(to_cw: &Sender<Message>, id: u16) {
    let mut fei = FrontendId{adapter: id, frontend: 0};
    loop {
        match fs::metadata(&frontend_path(&fei)) {
            Ok(m) => {
                // NB m.is_file() is false for special files. :-(
                // Assume the special devices were are dealing with are
                // character devices not block devices.
                if m.file_type().is_char_device() {
                    to_cw.send(Message::FrontendAppeared{fei: fei.clone()}).unwrap();
                }
            },
            Err(_) => { break; },
        };
        fei.frontend += 1;
    }
}

/// Search for any adapters already installed on start of the application
pub fn search_and_add_adaptors(to_cw: &Sender<Message>) {
    if fs::metadata(DVB_BASE_PATH).is_ok() {
        let mut adapter_number = 0;
        loop {
            if fs::metadata(adapter_path(adapter_number)).is_ok() {
                add_frontends(to_cw, adapter_number);
            } else { break; }
            adapter_number += 1;
        }
    }
}

/// The entry point for the thread that is the front end manager process.
pub fn run(from_in: Receiver<IN_Message>, to_cw: Sender<Message>) {
    search_and_add_adaptors(&to_cw);
    loop {
        match from_in.recv() {
            Ok(r) => {
                match r {
                  IN_Message::AdapterAppeared{id} => {
                      // The C++ version discovered that there was a delay between
                      // notification of the adapter creation and the accessibility of
                      // the frontend file(s). Delaying for 1s seemed to do the trick.
                      // Add this for the Rust version with a view to trying the experiment
                      // again to see if the delay is still required.
                      thread::sleep(time::Duration::from_secs(1));
                      add_frontends(&to_cw, id);
                  },
                  IN_Message::AdapterDisappeared{id} => {
                      to_cw.send(Message::AdapterDisappeared{id}).unwrap();
                  },
                }
            },
            Err(_) => {
                println!("Frontend Manager got an Err, so inotify end of channel has dropped..");
                break;
            },
        }
    }
    println!("Frontend Manager terminated.");
}

#[cfg(test)]
mod tests {
    use super::*;

    quickcheck! {
        fn adapter_path_is_correct(id: u16) -> bool {
            adapter_path(id) == format!("/dev/dvb/adapter{}", id)
        }
    }

    quickcheck! {
        fn frontend_path_is_correct(a: u16, f: u16) -> bool {
            frontend_path(&FrontendId{adapter: a, frontend: f}) == format!("/dev/dvb/adapter{}/frontend{}", a, f)
        }
    }

    quickcheck! {
        fn demux_path_is_correct(a: u16, f: u16) -> bool {
            demux_path(&FrontendId{adapter: a, frontend: f}) == format!("/dev/dvb/adapter{}/demux{}", a, f)
        }
    }

    quickcheck! {
        fn dvr_path_is_correct(a: u16, f: u16) -> bool {
            dvr_path(&FrontendId{adapter: a, frontend: f}) == format!("/dev/dvb/adapter{}/dvr{}", a, f)
        }
    }

}