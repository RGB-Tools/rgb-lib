## Transfer state diagrams
These diagrams show the states traversed by an asset transfer and their
relationships.

For the possible states a transfer can be in, please see the [`TransferStatus`
enum].

Issuance is not covered here, as it is created in status `Settled` right away
and doesn't go through any state change.

### Sender
From the sender's perspective, the main differences in flow are due to the
transfer being a default or a donation one and, in the default mode, due to the
confirmation from the receiver.

![sender](http://www.plantuml.com/plantuml/proxy?src=https://raw.githubusercontent.com/RGB-Tools/rgb-lib/master/docs/UML/transfer_states_sender.puml)

### Receiver
From the receiver's perspective, the main difference in flow is due to the
transfer consignment being successfully validated or not.

![receiver](http://www.plantuml.com/plantuml/proxy?src=https://raw.githubusercontent.com/RGB-Tools/rgb-lib/master/docs/UML/transfer_states_receiver.puml)


[`TransferStatus` enum]: https://docs.rs/rgb-lib/latest/rgb_lib/enum.TransferStatus.html
