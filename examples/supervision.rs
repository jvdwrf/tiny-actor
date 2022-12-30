use std::{collections::HashMap, sync::Arc, time::Duration};

use std::sync::Mutex;
use tiny_actor::*;
type Registered = Arc<Mutex<HashMap<&'static str, Address<Channel<Message>>>>>;

#[derive(Debug)]
enum Message {}

#[tokio::main]
async fn main() {
    // This is our map with registered actors
    let registered = Arc::new(Mutex::new(
        HashMap::<&'static str, Address<Channel<Message>>>::new(),
    ));

    // Now we spawn a new actor
    let (mut child, mut address) = spawn_actor();

    // Clone that which is shared between the supervisor and the main task
    let clone_address = address.clone();
    let clone_registered = registered.clone();

    // And spawn the supervisor task
    tokio::task::spawn(async move {
        // which registers the address under the name "MyAddress"
        register_address(&clone_registered, "MyAddress", clone_address);

        loop {
            // This supervisor waits until the child exits
            child.await.unwrap();
            println!("Supervisor -- spawning new actor.");
            // Spawns the actor again
            let (new_child, new_address) = spawn_actor();
            child = new_child;
            // And re-registers it's new address under the same name
            register_address(&clone_registered, "MyAddress", new_address);
            println!("Supervisor -- New actor registered! \n")
        }
    });

    loop {
        // Now, every second, we halt the child.
        address.halt();
        tokio::time::sleep(Duration::from_secs(1)).await;

        // It should have exited now.
        assert!(address.has_exited());

        // If we now look up the new address, we can see it registered.
        address = look_up_address(&registered, "MyAddress").unwrap();
        // and alive!
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
    spawn_process(Config::default(), |mut inbox| async move {
        loop {
            match inbox.recv().await {
                Ok(msg) => println!("Actor -- received message {:?}", msg),
                Err(e) => {
                    println!("Actor -- received error: \"{:?}\", exiting.", e);
                    break e;
                }
            }
        }
    })
}
