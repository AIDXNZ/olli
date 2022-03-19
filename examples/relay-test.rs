use crate::client::Event;
use crate::gossipsub::Gossipsub;
use async_std::io;
use clap::Arg;
use clap::Command;
use env_logger::{Builder, Env};
use futures::{prelude::*, select};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::{self};
use libp2p::core::upgrade::Version;
use libp2p::gossipsub::MessageId;
use libp2p::gossipsub::{
    GossipsubEvent, GossipsubMessage, IdentTopic as Topic, MessageAuthenticity, ValidationMode,
};
use libp2p::noise::{Keypair, NoiseConfig, X25519Spec};
use libp2p::swarm::NetworkBehaviourEventProcess;
use libp2p::Swarm;

use libp2p::tcp::TcpConfig;
use libp2p::yamux::YamuxConfig;
use libp2p::{gossipsub, identity, swarm::SwarmEvent, Multiaddr, PeerId};
use libp2p::{mplex, noise, Transport};
use multiaddr::multiaddr;
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::time::Duration;
//use libp2p_dcutr::dcutr;
use futures::executor::block_on;
use libp2p::identify::Identify;
use libp2p::relay::v2::client::{self, Client};
use tokio::{fs, io::AsyncBufReadExt, sync::mpsc};

pub fn build_transport(key_pair: identity::Keypair) -> transport::Boxed<(PeerId, StreamMuxerBox)> {
    let auth_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&key_pair)
        .unwrap();

    let transp = TcpConfig::new().nodelay(true);
    let noise_config = noise::NoiseConfig::xx(auth_keys).into_authenticated();
    let yamux_config = YamuxConfig::default();

    transp
        .upgrade(Version::V1)
        .authenticate(noise_config)
        .multiplex(yamux_config)
        .timeout(Duration::from_secs(10))
        .boxed()
}

use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p::NetworkBehaviour;
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Events")]
struct ComposedBehaviour {
    gossipsub: Gossipsub,
    //dcutr: dcutr::behaviour::Behaviour,
    //relay_client: Client,
}

#[derive(Debug)]
enum Events {
    Gossipsub(GossipsubEvent),
    //Dcutr(dcutr::behaviour::Event),
    //Relay(client::Event),
}

impl From<GossipsubEvent> for Events {
    fn from(e: GossipsubEvent) -> Self {
        Events::Gossipsub(e)
    }
}

// impl From<client::Event> for Event {
//     fn from(e: client::Event) -> Self {
//         Event::Relay(e)
//     }
// }

// impl From<dcutr::behaviour::Event> for Event {
//     fn from(e: dcutr::behaviour::Event) -> Self {
//         Event::Dcutr(e)
//     }
// }

impl NetworkBehaviourEventProcess<GossipsubEvent> for ComposedBehaviour {
    fn inject_event(&mut self, event: GossipsubEvent) {
        match event {
            GossipsubEvent::Message {
                propagation_source: peer_id,
                message_id: id,
                message,
            } => {
                println!("Got message");
            }
            _ => (),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    Builder::from_env(Env::default().default_filter_or("info")).init();
    //Parse Args
    let matches = Command::new("Olli")
        .version("0.0.1")
        .author("Aidan McComas")
        .arg(
            Arg::new("connect")
                .help("Connect to peer with provided Address")
                .required(true),
        )
        .get_matches();

    // Create a random PeerId
    let local_key = identity::Keypair::generate_ed25519();

    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    // Set up an encrypted TCP Transport over the Mplex and Yamux protocols
    let transport = build_transport(local_key.clone());

    // TODO
    //let (relay_transport, client) = Client::new_transport_and_behaviour(local_peer_id);

    // Create a Swarm with the beahvior we provided
    let mut swarm = {
        // Create a Gossipsub topic
        let topic = Topic::new("bruh");
        // Function for getting the hash of each message
        let message_id_fn = |message: &GossipsubMessage| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            MessageId::from(s.finish().to_string())
        };

        // Create Gossipsub configuration
        let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
            .validation_mode(ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
            .message_id_fn(message_id_fn) // content-address messages. No two messages of the
            // same content will be propagated.
            .build()
            .expect("Valid config");
        //Create Gossipsub with our config and message_id_fn
        let mut gossipsub =
            Gossipsub::new(MessageAuthenticity::Signed(local_key), gossipsub_config)
                .expect("Able to Gossip");
        gossipsub.subscribe(&topic).expect("Subscribed to Topic");

        //Custom behavior with the ComposedBehaviour struct
        let behaviour = ComposedBehaviour {
            gossipsub: gossipsub,
            //dcutr: dcutr::behaviour::Behaviour::new(),
            //relay_client: client,
        };

        Swarm::new(transport, behaviour, local_peer_id)
    };

    // Listen on all interfaces and whatever port the OS assigns
    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    // Reach out to another node if specified
    // if let Some(to_dial) = matches.value_of("connect") {
    //     let address: Multiaddr = to_dial.parse().expect("User to provide valid address.");
    //     match swarm.dial(address.clone()) {
    //         Ok(_) => println!("Dialed {:?}", address),
    //         Err(e) => println!("Dial {:?} failed: {:?}", address, e),
    //     };
    // }

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines().fuse();

    if matches.value_of("connect").is_some() {
        let addr: Multiaddr = matches.value_of("connect").unwrap().parse().unwrap();
        if let Err(e) = swarm.dial(addr) {
            println!("Publish error: {:?}", e);
        }
    }

    // Kick it off
    loop {
        select! {
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening on {:?}", address);
                }
                SwarmEvent::Dialing(local_peer_id) => {
                    println!("Dialiang Peer: {:?}", local_peer_id)
                }
                SwarmEvent::ConnectionEstablished {endpoint, ..} => {
                    println!("Connected to peer on {:?}", endpoint)
                }
                SwarmEvent::IncomingConnection {local_addr, send_back_addr} => {
                    println!("Incoming Connection on {:?} to {:?}", local_addr, send_back_addr)
                }
                _ => {}
            }
        }
    }
}
