---
title: Additional Validator Info
---

# Additional Validator Info

This proposal describes HTTP-based retrieval of off-chain validator info.

The term _validator info_ refers to various metadata associated with a validator's vote authority, including a web URL, a Keybase username, a description, and an image.

## Motivation

### Unauthenticated `website` attribute

Validator info accounts (config program) optionally allow specifying a web URL in the `website` attribute.

There is no accepted convention on how to verify that the location pointed to by the `website` attribute is actually controlled by the validator operator.
This allows any validator to display any arbitrary website on validator info aggregators, which is potentially misleading to stakers.

### Associated Image

Keybase profile pictures have become the de-facto standard to set the logo of a validator.
This mechanism is currently undocumented.

The validator info standard should define a mechanism to discover media that does not rely on a single external provider.

## Proposed Solution

### HTTP resource discovery

The `website` URL already points to a validator-controlled off-chain location.
This makes for a useful mechanism for providing additional info.

[RFC 8615] defines a convention to discover information from arbitrary origins in a consistent manner.
The origin ([RFC 6454]) is derived from the FQDN of an URL. The scheme is always set to `https://`.
Validators should accordingly expose additional info under absolute path `/.well-known/solana/validator-<pubkey>`.

[RFC 6454]: https://www.rfc-editor.org/rfc/rfc6454
[RFC 8615]: https://www.rfc-editor.org/rfc/rfc8615

For example, given website `https://example.org/index.html` and validator authorized voter (identity) `123ijcpx42kDEgc4CvBXvhu4wjeFXxb9j1JhfJVWTs5f`,
the info URL is as follows.

```
https://example.org/.well-known/solana/validator-123ijcpx42kDEgc4CvBXvhu4wjeFXxb9j1JhfJVWTs5f
```

The server should return HTTP status "200 OK" if the validator info file was found, and otherwise "404 Not Found".
The client should follow redirects.
The existence of the validator info file implies that the website is acknowledging its association with the given validator,
making it suitable for verifying the `website` attribute stored on-chain.

The media type must be `application/json`.
Clients should reject files larger than 100 KB and limit the request duration to avoid slowloris attacks.

**JSON schema**

```yaml
$ref: "#/definitions/AdditionalValidatorInfo"
definitions:
  AdditionalValidatorInfo:
    type: object
    properties:
      country:
        type: string
        description: ISO 3166-1 two-letter country code
      icon-360x360:
        type: string
        description: URL to the validator icon
```

**Empty additional info**

If the validator only wants to prove website ownership without specifying data,
the file content should be set to an empty JSON object (curly braces).

```json
{}
```

**Example additional info**

```json
{
  "country": "de",
  "icon-360x360": "https://media.example.org/icon.webp"
}
```

### Validator icon

An URL to the validator icon can be set in info key `icon-360x360`.
Supported formats are `.webp`, `.png`, or `.jpg`.
The icon must have the size 360x360 px.

The use of Keybase profile pictures is deprecated.

### Further considerations

We expect the additional info schema to grow over time.

Further potential use-cases include the following:
 - Additional media such as icons of different sizes.
 - Specifying contact / security details.
 - A list of Discord usernames associated with the validator to authorize access to operator-only chat rooms.
 - Feature flags for consensus mods, MEV, alternative staking rewards
