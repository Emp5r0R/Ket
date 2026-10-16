# invite-capsule

One signed invite format, shared by [Ket](https://github.com/Emp5r0R/Ket) and
[Abyssal](https://github.com/Emp5r0R/Abyssal).

An invite is a short text (or QR code) that carries where to reach a node, the
node's Ed25519 public key, and a secret capability. The node signs it, so a
client can check it was not changed and pin the node's key before sending
anything.

## Using it

Each application implements `InviteProfile`: its application ID, signature
domain, capability type, the locator kinds it accepts, and its text prefix
(plus an optional hand-typeable form). Everything else is shared:

- canonical CBOR, re-encoded on decode so every invite has exactly one
  binary form;
- the payload layout and signature input;
- strict Ed25519 verification, checked before expiry;
- locators: public HTTPS names, development loopback, Tor v3 onion, I2P;
- `<prefix><base64url>` text and the checksummed Crockford manual form.

An invite made for one profile is rejected by every other profile.

The crate does no networking, storage, or UI.

## Using it from another project

It is not published to crates.io. Depend on it by git, pinned to a commit:

```toml
invite-capsule = { git = "https://github.com/Emp5r0R/Ket", rev = "<commit>" }
```

## Licence

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT),
at your option.
