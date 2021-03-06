/*
 *  Me TV — It's TV for me computer.
 *
 *  A GTK+/GStreamer client for watching and recording DVB.
 *
 *  Copyright © 2018–2020  Russel Winder
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

use std::{thread, time};

use clap::{Arg, App};

use gst::{gst_element_error, gst_element_warning};
use gst::prelude::*;

fn main() {
    let matches = App::new("me-tv-record")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Russel Winder <russel@winder.org.uk>")
        .about("Record a channel from now for a duration to create an MPEG4 file.

A channel name and a duration must be provided.
")
        .arg(Arg::with_name("adapter")
            .short("a")
            .long("adapter")
            .value_name("NUMBER")
            .help("Sets the adapter number to use.")
            .takes_value(true)
            .default_value("0"))
        .arg(Arg::with_name("frontend")
            .short("f")
            .long("frontend")
            .value_name("NUMBER")
            .help("Sets the frontend number to use.")
            .takes_value(true)
            .default_value("0"))
        .arg(Arg::with_name("channel")
            .short("c")
            .long("channel")
            .value_name("CHANNEL")
            .help("Sets the channel name, must be specified, no default.")
            .takes_value(true)
            .required(true))
        .arg(Arg::with_name("duration")
            .short("d")
            .long("duration")
            .value_name("TIME")
            .help("Sets the duration of recording in minutes, must be specified, no default.")
            .takes_value(true)
            .required(true))
        .arg(Arg::with_name("output")
            .short("o")
            .long("output")
            .value_name("PATH")
            .help("Path to output file, must be specified, no default.")
            .takes_value(true)
            .required(true))
        .arg(Arg::with_name("verbose")
            .short("v")
            .long("verbose")
            .help("sets verbose mode"))
        .get_matches();
    let be_verbose = matches.is_present("verbose");
    let adapter = matches.value_of("adapter").unwrap().parse::<u8>().expect("Couldn't parse adapter value as a positive integer.");
    let frontend = matches.value_of("frontend").unwrap().parse::<u8>().expect("Couldn't parse frontend value as a positive integer.");
    let channel = matches.value_of("channel").unwrap();
    let duration = matches.value_of("duration").unwrap().parse::<u32>().expect("Couldn't parse the provided duration as a positive integer.");
    let output_path = matches.value_of("output").unwrap();
    if be_verbose {
        println!("Recording channel '{}' for {} minutes on adapter {} frontend {}.", channel, duration, adapter, frontend);
    }
    //
    // Construct the GStreamer graph described by:
    //
    //    gst-launch-1.0 -e uridecodebin uri=dvb://<channel> name=d ! queue ! x264enc ! mp4mux name=m ! filesink location=<output-path> d. ! queue ! avenc_ac3 ! m.
    //
    gst::init().unwrap();
    let pipeline = gst::Pipeline::new(None);
    let uridecodebin = {
        let element = gst::ElementFactory::make("uridecodebin", None).expect("cannot make uridecodebin");
        element.set_property("uri", &format!("dvb://{}", channel)).expect("cannot set uri property on uridecodebin");
        element.connect("source-setup",  false, {
            let adapter_number = adapter;
            let frontend_number = frontend;
            move |values| {
                // values[0] .get::<gst::Element>() is an Option on the uridecodebin itself.
                let element = values[1].get::<gst::Element>()
                    .expect("Failed to get a handle on the Element being created")
                    .expect("Option on Element was None");
                if let Some(element_factory) = element.get_factory() {
                    if element_factory.get_name() == "dvbbasebin" {
                        let current_adapter_number = element
                            .get_property("adapter")
                            .expect("Could not retrieve adapter number Value")
                            .get::<i32>()
                            .expect("Could not get the i32 value from the adapter number Value")
                            .expect("Option on u32 returned None") as u8;
                        let current_frontend_number = element
                            .get_property("frontend")
                            .expect("Could not retrieve frontend number Value.")
                            .get::<i32>()
                            .expect("Could not get the i32 value from the frontend number Value")
                            .expect ("Option on u32 returned None") as u8;
                        if current_adapter_number != adapter_number {
                            element.set_property("adapter", &(adapter_number as i32)).expect("Could not set adapter number on dvbsrc element");
                        }
                        if current_frontend_number != adapter_number {
                            element.set_property("frontend", &(frontend_number as i32)).expect("Could not set frontend number of dvbsrc element");
                        }
                    }
                }
                None
            }
        }).expect("Could not connect a handler to the source-setup signal.");
        element
    };
    let mp4mux = gst::ElementFactory::make("mp4mux", None).expect("cannot make mp4mux");
    let filesink = {
        let element = gst::ElementFactory::make("filesink", None).expect("cannot make filesrc");
        element.set_property("location", &output_path).expect("cannot set location for filesrc");
        element
    };
    pipeline.add_many(&[&uridecodebin, &mp4mux, &filesink]).expect("could not add elements to pipeline");
    gst::Element::link_many(&[&mp4mux, &filesink]).expect("could not link elements in pipeline");
    // Heed the warnings about strong references, circular references and memory leaks.
    let pipeline_weak_ref = pipeline.downgrade();
    uridecodebin.connect_pad_added(move |d_b, src_pad| {
        let pipeline = match pipeline_weak_ref.upgrade() {
            Some(pipeline) => pipeline,
            None => return,
        };
        let (is_audio, is_video) = {
            let media_type = src_pad.get_current_caps().and_then(|caps| {
                caps.get_structure(0).map(|s| {
                    let name = s.get_name();
                    (name.starts_with("audio/"), name.starts_with("video/"))
                })
            });
            match media_type {
                Some(media_type) => media_type,
                None => {
                    gst_element_warning!(d_b, gst::CoreError::Negotiation, ("Failed to get media type from pad {}", src_pad.get_name()));
                    return;
                },
            }
        };
        let insert_sink = |is_audio, is_video| -> Result<(), ()> {
            if is_audio && is_video { panic!("sink is both audio and video at the same time"); }
            if ! is_audio && ! is_video { return Ok(()); }
            let queue = gst::ElementFactory::make("queue", None).expect("cannot make a queue");
            let new_element = if is_audio {
                gst::ElementFactory::make("avenc_ac3", None).expect("cannot make a avenc_ac3")
            } else {
                gst::ElementFactory::make("x264enc", None).expect("cannot make a x264enc")
            };
            let elements = &[&queue, &new_element];
            pipeline.add_many(elements).expect("could not add elements to pipeline");
            gst::Element::link_many(elements).expect("could not link elements in pipeline");
            for e in elements {
                e.sync_state_with_parent().expect("could not sync state of elements with parent");
            }
            let sink_pad = queue.get_static_pad("sink").expect("video queue has no sink pad");
            src_pad.link(&sink_pad).expect("linking src_pad to sink_pad of new queue failed");
            let new_element_src_pad = new_element.get_static_pad("src").expect("new element has no src pad");
            let sink_pad_template = if is_audio { "audio_%u" } else { "video_%u" };
            let mp4mux_sink_pad = mp4mux.get_request_pad(sink_pad_template).expect(&format!("mp4mux has no {} sink pad", sink_pad_template));
            new_element_src_pad.link(&mp4mux_sink_pad).expect("linking new element to mp4mux failed.");
            Ok(())
        };
        if let Err(err) = insert_sink(is_audio, is_video) {
            //  TODO why are the parentheses needed around the string?
            gst_element_error!(d_b, gst::LibraryError::Failed, ("Failed to insert sink"), ["{:?}", err]);
        }
    });
    pipeline.set_state(gst::State::Playing).unwrap();
    thread::spawn({
        let pipeline_weak_ref = pipeline.downgrade();
        move || {
            thread::sleep(time::Duration::from_secs((duration * 60).into()));
            let pipeline = match pipeline_weak_ref.upgrade() {
                Some(pipeline) => pipeline,
                None => panic!("no access to the pipeline"),
            };
            pipeline.send_event(gst::event::Eos::new());
        }
    });
    ctrlc::set_handler({
        let pipeline_weak_ref = pipeline.downgrade();
        move || {
            let pipeline = match pipeline_weak_ref.upgrade() {
                Some(pipeline) => pipeline,
                None => panic!("no access to the pipeline"),
            };
            pipeline.send_event(gst::event::Eos::new());
        }
    }).expect("Error setting ctrl-c handler.");
    let bus = pipeline.get_bus().expect("Pipeline without bus. Shouldn't happen!");
    while let Some(msg) = bus.timed_pop(gst::CLOCK_TIME_NONE) {
        use gst::MessageView;
        match msg.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                pipeline.set_state(gst::State::Null).unwrap();
                println!("Error: {} {} {} {}",
                         err.get_src().map(|s| s.get_path_string()).unwrap_or_else(|| glib::GString::from("None")),
                         err.get_error().to_string(),
                         err.get_debug().unwrap_or_else(|| String::from("None")),
                         err.get_error(),
                );
                break
            },
            MessageView::StateChanged(s) => {
                if be_verbose {
                    println!(
                        "State changed from {:?}: {:?} -> {:?} ({:?})",
                        s.get_src().map(|s| s.get_path_string()),
                        s.get_old(),
                        s.get_current(),
                        s.get_pending()
                    );
                }
            }
            _ => (),
        }
    }
    pipeline.set_state(gst::State::Null).unwrap();
}
