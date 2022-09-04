# tiny-actor
[![Crates.io](https://img.shields.io/crates/v/tiny-actor)](https://crates.io/crates/tiny-actor)
[![Documentation](https://docs.rs/tiny-actor/badge.svg)](https://docs.rs/tiny-actor)

Tiny-actor is a minimal and unopinionated actor library for Rust.

This library merges the concepts of `Inboxes` and `tasks`, which results in actors: This basic building-block allows us to build simple pools and supervision-trees with reliable shutdown behaviour.

Tiny-actor will not be going for more advanced API's, but acts as a simple way to write well-behaving tokio-actors. (as nicely explained [here](https://ryhl.io/blog/actors-with-tokio/)) 

If you're looking for a fully-fledged actor-framework, then please take a look at [Zestors](https://github.com/Zestors/zestors). It builds further on building blocks of tiny-actor.

# Concepts
The following gives a quick overview of all concepts of tiny-actor. For more detailed information about usage, please refer to the crate [documentation](https://docs.rs/tiny-actor).

## Channel
A `Channel` is that which couples `Inboxes`, `Addresses` and `Children` together. Every `Channel` contains the following rust-structs: 
* A single `Child(Pool)`.
* One or more `Addresses`.
* One or more `Inboxes`.

The following diagram shows a visual representation of the naming used:
```other
|¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|
|                            Channel                          |
|  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |
|  |              actor                |  |   Child(Pool)  |  |
|  |  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |  |________________|  |
|  |  |         process(es)         |  |                      |
|  |  |  |¯¯¯¯¯¯¯¯¯¯|  |¯¯¯¯¯¯¯¯¯|  |  |  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |
|  |  |  |   task   |  |  Inbox  |  |  |  |  Address(es)   |  |
|  |  |  |__________|  |_________|  |  |  |________________|  |
|  |  |_____________________________|  |                      |
|  |___________________________________|                      |
|_____________________________________________________________|
```

## Actor
The term `actor` is used to describe a group of `processes` belonging to a single `Channel`.

## Process
The term `process` is used to describe the a `task` paired with an `Inbox`. 

## Inbox
An `Inbox` is a receiver to the `Channel`, and is primarily used to take messages out of the `Channel`. `Inboxes` can be created by spawning new `processes` and should stay coupled to the `task` they were spawned with: An `Inbox` should only be dropped when the `task` is exiting.

## Address
An `Address` is the clone-able sender of a `Channel`, and is primarily used to send messages to the `actor`. `Addresses` can be awaited, which returns once the `actor` exits.

## Child(Pool)
A `Child` is a handle to an `actor` consisting of one `process`. It can be awaited to return the exit-value of the spawned `task`. The `Child` is not clone-able, and therefore unique to the `Channel`. When it is dropped, the `actor` will be `halted` and subsequently `aborted`, this behaviour can be by detaching the `Child`. 

A `ChildPool` is similar to a `Child`, except that the `actor` consist of multiple `processes`. The `ChildPool` can be streamed to get the exit-values of all spawned `tasks`. More `processes` can be spawned after the `actor` has been spawned, and it's also possible to `halt` a portion of the `processes` of the `actor`.

## Closing
Once `Channel` is `closed`, it is not longer possible to send new messages into it, it is still possible to take out any messages that are left. The `processes` of a closed `Channel` do not have to exit necessarily, but can continue running. Any senders are notified with a `SendError::Closed`, while receivers will receive `RecvError::ClosedAndEmpty` once the `Channel` has been emptied.

## Halting
A `process` can be `halted` exactly once, by receiving a `RecvError::Halted`, after which it should exit. An `actor` can be partially halted, meaning that only some of it's `processeses` have been `halted`.

## Aborting
An `actor` can be `aborted` through tokio's [abort](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html#method.abort) method. This causes the `tasks` to exit abruptly, and can leave bad state behind, wherever possible, use `halt` instead of `abort`.

## Exiting
An `exit` can refer to two seperate events which, with good practise, always occur at the same time:
* A `process` can exit by dropping it's `Inbox`, once all `Inboxes` of a `Channel` have been dropped the `actor` has `exited`. This type of exit can be retrieved from the `Channel` at any time using `has_exited`.
* A `task` can exit, which means the `task` is no longer alive. This can only be queried from the `Child(Pool)` by awaiting it or by calling `is_finished`. 

## Link
An `actor` can either be `attached` or `detached`, which indicates what should happen when the `Child(Pool)` is dropped:
 * If it is `attached` then it will automatically `halt` all `processes`. After the `abort-timer` expires all processes will be `aborted`. 
 * If it is `detached`, then nothing happens when the `Child(Pool)` is dropped.

## Capacity
A `Channel` can either be `bounded` or `unbounded`. 
* A bounded `Channel` can receive messages until it's capacity has been reached. After reaching the capacity, senders must wait until space is available. 
* An unbounded `Channel` does not have this limit, but instead applies a backpressure-algorithm: The more messages in the `Channel`, the longer the sender must wait before it is allowed to send. 

## Id
Every actor has a unique id generated when it is spawned, this id can not be changed after it's creation.

# Getting started

## Basic example
```rust
use std::time::Duration;
use tiny_actor::*;

#[tokio::main]
async fn main() {
    // First we spawn an actor with a default config, and an inbox which receives u32 messages.
    let (mut child, address) = spawn(Config::default(), |mut inbox: Inbox<u32>| async move {
        loop {
            // This loops and receives messages
            match inbox.recv().await {
                Ok(msg) => println!("Received message: {msg}"),
                Err(error) => match error {
                    RecvError::Halted => {
                        println!("actor has received halt signal - Exiting now...");
                        break "Halt";
                    }
                    RecvError::ClosedAndEmpty => {
                        println!("Channel is closed - Exiting now...");
                        break "Closed";
                    }
                },
            }
        }
    });

    // Then we can send it messages
    address.send(10).await.unwrap();
    address.send(5).await.unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;

    // And finally shut the actor down, 
    // we give it 1 second to exit before aborting it.
    match child.shutdown(Duration::from_secs(1)).await {
        Ok(exit) => {
            assert_eq!(exit, "Halt");
            println!("actor exited with message: {exit}")
        }
        Err(error) => match error {
            ExitError::Panic(_) => todo!(),
            ExitError::Abort => todo!(),
        },
    }
}

```

## Example with ChildPool and custom Config
```rust
use futures::stream::StreamExt;
use std::time::Duration;
use tiny_actor::*;

#[tokio::main]
async fn main() {
    // First we spawn an actor with a custom config, and an inbox which receives u32 messages.
    // This will spawn 3 processes, with i = {0, 1, 2}.
    let (mut pool, address) = spawn_many(
        0..3,
        Config {
            link: Link::Attached(Duration::from_secs(1)),
            capacity: Capacity::Unbounded(BackPressure::exponential(
                5,
                Duration::from_nanos(25),
                1.3,
            )),
        },
        |i, mut inbox: Inbox<u32>| async move {
            loop {
                // Now every actor loops in the same way as in the basic example
                match inbox.recv().await {
                    Ok(msg) => println!("Received message on actor {i}: {msg}"),
                    Err(error) => match error {
                        RecvError::Halted => {
                            println!("actor has received halt signal - Exiting now...");
                            break "Halt";
                        }
                        RecvError::ClosedAndEmpty => {
                            println!("Channel is closed - Exiting now...");
                            break "Closed";
                        }
                    },
                }
            }
        },
    );

    tokio::time::sleep(Duration::from_millis(10)).await;

    // Send it the numbers 0..10, they will be spread across all processes.
    for num in 0..10 {
        address.send(num).await.unwrap()
    }

    // And finally shut the actor down, giving it 1 second before aborting.
    let exits = pool
        .shutdown(Duration::from_secs(1))
        .collect::<Vec<_>>() // Await all processes (using `futures::StreamExt::collect`)
        .await;

    // And assert that every exit is `Ok("Halt")`
    for exit in exits {
        match exit {
            Ok(exit) => {
                assert_eq!(exit, "Halt");
                println!("actor exited with message: {exit}")
            }
            Err(error) => match error {
                ExitError::Panic(_) => todo!(),
                ExitError::Abort => todo!(),
            },
        }
    }
}

```