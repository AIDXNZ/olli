use async_std::io;
use env_logger::{Builder, Env};
use futures::{prelude::*, select};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::{self, upgrade};
use libp2p::core::upgrade::Version;
use libp2p::gossipsub::MessageId;
use libp2p::gossipsub::{
    GossipsubEvent, GossipsubMessage, IdentTopic as Topic, MessageAuthenticity, ValidationMode,
};
use libp2p::pnet::PnetConfig;
use libp2p::yamux::YamuxConfig;
use libp2p::{mplex, noise, Transport};
use libp2p::noise::{X25519Spec, Keypair, NoiseConfig};
use libp2p::tcp::TcpConfig;
use libp2p::{gossipsub, identity, swarm::SwarmEvent, Multiaddr, PeerId};
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::time::Duration;


pub fn build_transport(
    key_pair: identity::Keypair,
) -> transport::Boxed<(PeerId, StreamMuxerBox)> {
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
            .timeout(Duration::from_secs(20))
            .boxed()

}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    Builder::from_env(Env::default().default_filter_or("info")).init();

    // Create a random PeerId
    let local_key = identity::Keypair::generate_ed25519();

    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    // Set up an encrypted TCP Transport over the Mplex and Yamux protocols
    let transport = build_transport(local_key.clone());
    // Create a Gossipsub topic
    let topic = Topic::new("test");

    // Create a Swarm to manage peers and events
    let mut swarm = {
        // To content-address message, we can take the hash of message and use it as an ID.
        let message_id_fn = |message: &GossipsubMessage| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            MessageId::from(s.finish().to_string())
        };

        // Set a custom gossipsub
        let gossipsub_config = gossipsub::GossipsubConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10)) // This is set to aid debugging by not cluttering the log space
            .validation_mode(ValidationMode::Strict) // This sets the kind of message validation. The default is Strict (enforce message signing)
            .message_id_fn(message_id_fn) // content-address messages. No two messages of the
            // same content will be propagated.
            .build()
            .expect("Valid config");
        // build a gossipsub network behaviour
        let mut gossipsub: gossipsub::Gossipsub =
            gossipsub::Gossipsub::new(MessageAuthenticity::Signed(local_key), gossipsub_config)
                .expect("Correct configuration");

        // subscribes to our topic
        gossipsub.subscribe(&topic).unwrap();


        // build the swarm
        libp2p::Swarm::new(transport, gossipsub, local_peer_id)
    };

    // Listen on all interfaces and whatever port the OS assigns
    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    // Reach out to another node if specified

    // Read full lines from stdin
    let mut stdin = io::BufReader::new(io::stdin()).lines().fuse();

    // Kick it off
    loop {
        select! {
            line = stdin.select_next_some() => {
                let addr: Multiaddr = line.unwrap().parse().expect("User Provided Addr");
                swarm.dial(addr.clone());
                
            },
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(GossipsubEvent::Message {
                    propagation_source: peer_id,
                    message_id: id,
                    message,
                }) => println!(
                    "Got message: {} with id: {} from peer: {:?}",
                    String::from_utf8_lossy(&message.data),
                    id,
                    peer_id
                ),
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening on {:?}", address);
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