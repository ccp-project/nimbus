CCP Algorithm: Nimbus
=====================

This repository provides a CCP implementation of the Nimbus elasticity detection algorithm described
[here](https://arxiv.org/pdf/1802.08730.pdf). The elasticity detector is not a congestion control
scheme in itself, but rather a mechanism for detecting whether the competing cross traffic on
a bottleneck link is elastic or inelastic and switching between congestion control schemes to 
compete appropriately. 

To get started using this algorithm with CCP, please see our [guide](https://ccp-project.github.io/guide).

This implementation uses TCP Cubic when it detects that any of the background traffic is elastic,
and a custom delay control rule otherwise. However, Nimbus is agnostic to the underlying CC algorithms
and could be used with any other loss-based and delay-based scheme, respectively. 

Please see the paper for more details.
If you have any questions, please contact us at nimbus@nms.csail.mit.edu.
