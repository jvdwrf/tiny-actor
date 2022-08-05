use std::{collections::HashMap, sync::Arc, time::Duration};

use std::sync::Mutex;
use tiny_actor::*;

type Registered = Arc<Mutex<HashMap<&'static str, Address<Channel<Message>>>>>;

#[derive(Debug)]
enum Message {}

#[tokio::main]
async fn main() {
    let (child, mut address) = spawn_actor();

    let registered = Arc::new(Mutex::new(
        HashMap::<&'static str, Address<Channel<Message>>>::new(),
    ));

    let clone_address = address.clone();
    let clone_registered = registered.clone();
    tokio::task::spawn(async move {
        let mut child = child;
        register_address(&clone_registered, "MyAddress", clone_address);

        loop {
            child.await.unwrap();
            println!("Supervisor -- spawning new actor now...");
            let (new_child, new_address) = spawn_actor();
            child = new_child;
            register_address(&clone_registered, "MyAddress", new_address);
            println!("Supervisor -- new actor registered!")
        }
    });



    loop {
        
        address.clone().await;
        tokio::time::sleep(Duration::from_secs(1)).await;
        address.clone().await;


        assert!(address.has_exited());
        address = look_up_address(&registered, "MyAddress").unwrap();
        assert!(!address.has_exited())
    }
}

fn register_address(
    registered: &Registered,
    name: &'static str,
    address: Address<Channel<Message>>,
) {
    registered.lock().unwrap().insert(name, address);
}

fn look_up_address(registered: &Registered, name: &str) -> Option<Address<Channel<Message>>> {
    registered.lock().unwrap().remove(name)
}

fn spawn_actor() -> (
    Child<RecvError, Channel<Message>>,
    Address<Channel<Message>>,
) {
    spawn(Config::default(), |mut inbox| async move {
        loop {
            match inbox.recv().await {
                Ok(msg) => println!("Actor -- received message {:?}", msg),
                Err(e) => {
                    println!("Actor -- received error {:?}, exiting now...", e);
                    break e;
                },
            }
        }
    })
}
