========================
 Rust TCP load balancer
========================

.. image:: https://travis-ci.org/mhallin/loadbalancer-rs.svg?branch=master
   :target: https://travis-ci.org/mhallin/loadbalancer-rs

A simple and straight-forward load balancer for TCP connections,
written in Rust. A sample configuration file shows off the current
features:

* Any number of backends that will perform round-robin load balancing
  over a number of target addresses.
* Any number of frontends listening on a port and forwarding all
  requests to a single backend.

The load balancer is built on top of the mio_ library, which provides
a fast and memory-efficient event driven architecture.

----

Trying it out
=============

Run it from a cloned copy of this repository. To run the included sample configuration, simply type:

.. code-block:: sh

   cargo run -- -c sample_config.toml


.. _mio: https://github.com/carllerche/mio
