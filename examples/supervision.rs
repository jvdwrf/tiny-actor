use std::{collections::HashMap, sync::Arc, time::Duration};

use std::sync::Mutex;
use tiny_actor::*;

type Registered = Arc<Mutex<HashMap<&'static str, Address<Channel<Message>>>>>;

#[derive(Debug)]
enum Message {}

#[tokio::main]
async fn main() {
    for _ in 0..10000 {
        let (child, address) = spawn(Config::default(), |mut inbox: Inbox<Message>| async move {
            loop {
                match inbox.recv().await {
                    Ok(_) => (),
                    Err(e) => {
                        println!("Received halt");
                        break e;
                    }
                }
            }
        });
        spin_sleep::sleep(Duration::from_nanos(100));
        println!("sending halt...");
        address.halt();
        address.await;
    }
}
