# ADR-002: Identity Provider Selection

**Status:** Accepted  
**Date:** 2024-01-20  
**Decision Makers:** Platform / Backend team

---

## Context

Haiker requires user authentication via OpenID Connect (OIDC) so that hikers can sign in, own their data, and access personalized features. The technology-stack decision established that a managed OIDC provider should be used rather than self-hosting an identity server.

We need to choose a specific managed provider for the MVP launch while keeping the option to switch later if business or pricing conditions change.

### Requirements

- **Standards-compliant OIDC:** The provider must implement the OpenID Connect Core specification so our integration uses standard flows (Authorization Code + PKCE).
- **No infrastructure to operate:** The team should not be responsible for patching, scaling, or monitoring an identity server.
- **Free tier adequate for MVP:** The provider must support the expected MVP user base (low thousands) without incurring cost.
- **Reversibility:** The choice must not create deep coupling that makes switching providers a rewrite.

---

## Decision

We select **Auth0** (free tier) as the managed OIDC identity provider for Haiker v1.

### Rationale

1. **Free tier covers MVP scale.** Auth0's free plan supports up to 25,000 monthly active users, which far exceeds projected MVP usage and removes cost pressure during the validation phase.
2. **Mature, standards-compliant OIDC implementation.** Auth0 has been in production for over a decade, implements the full OIDC spec, supports PKCE, and provides well-documented discovery endpoints. This reduces integration risk and debugging time.
3. **Zero infrastructure to operate.** Auth0 is fully managed SaaS. There are no containers to deploy, no databases to back up, no security patches to apply, and no scaling decisions to make.
4. **Ecosystem and documentation.** Extensive SDKs, guides, and community resources reduce onboarding friction, though our integration relies only on standard OIDC endpoints rather than vendor-specific SDKs.

### Provider-Agnostic Boundary

The domain-level `OidcProvider` trait in `crates/app/src/identity.rs` defines the authentication interface using only basic types (`String`, `Result`). The trait is explicitly infrastructure-free: implementations handle HTTP calls internally, but the application layer depends only on the trait contract (`authorization_url()` and `exchange_code()`).

This means Auth0-specific code lives exclusively in the infrastructure layer (one adapter implementing `OidcProvider`). Switching providers later is a contained change - replace the adapter, update configuration - not a rewrite of application or domain logic.

---

## Alternatives Considered

### 1. Self-Hosted Keycloak

**Pros:** Full control, feature-rich (federation, fine-grained authorization, admin console), open-source, no per-user pricing.

**Rejected because:**
- Requires deploying and operating a JVM-based server (memory-heavy, needs PostgreSQL, requires ongoing patching).
- Significant operational burden for a small team during MVP. Contradicts the technology-stack decision to use managed services for undifferentiated infrastructure.
- Scaling and high-availability configuration adds complexity that is not justified at current user counts.

### 2. Self-Hosted Ory (Hydra + Kratos)

**Pros:** Lightweight Go binaries, cloud-native design, modular (separate services for login, consent, identity).

**Rejected because:**
- Still requires operating multiple services (Hydra for OAuth2, Kratos for identity), each with their own database and configuration.
- Smaller community than Keycloak; fewer battle-tested production deployments at scale.
- The operational overhead contradicts the managed-provider decision, even though per-service resource usage is modest.

### 3. Self-Hosted Zitadel

**Pros:** Single binary, built-in multi-tenancy, modern UI, Go-based.

**Rejected because:**
- Relatively newer project with a smaller ecosystem and fewer production references.
- Still requires self-hosting infrastructure (database, TLS, monitoring, upgrades).
- Same fundamental objection as other self-hosted options: operational burden without proportional benefit at MVP scale.

### 4. Clerk

**Pros:** Developer-friendly APIs, pre-built UI components, fast integration for frontend-heavy apps.

**Rejected because:**
- Oriented toward frontend frameworks (React, Next.js) with opinionated UI components. Haiker's architecture is a Rust backend with a separate mobile client, so Clerk's frontend-first value proposition does not apply.
- Smaller free tier and less mature OIDC standards compliance compared to Auth0.
- Vendor-specific APIs would increase coupling beyond what a standard OIDC integration requires.

### 5. WorkOS

**Pros:** Enterprise SSO focus, clean API, directory sync (SCIM) built in.

**Rejected because:**
- Optimized for B2B enterprise SSO (SAML federation, directory sync) rather than B2C consumer authentication.
- Pricing is oriented toward enterprise contracts; the free tier is limited and not designed for consumer-scale MAU.
- Features like directory sync and admin portal are not needed for Haiker's consumer use case.

### 6. AWS Cognito

**Pros:** Generous free tier (50,000 MAU), tight integration with AWS services, managed and scalable.

**Rejected because:**
- Haiker is deployed on self-managed infrastructure (Docker Compose), not AWS. Using Cognito would introduce an AWS dependency without leveraging the broader AWS ecosystem.
- Cognito's OIDC implementation has known quirks (non-standard token claims, limited customization of hosted UI) that increase integration friction.
- Vendor lock-in to AWS identity services without corresponding infrastructure benefits.

---

## Consequences

### Positive

- **Zero operational overhead** for identity management during MVP and growth phases (up to 25k MAU).
- **Fast integration** using standard OIDC flows against well-documented endpoints.
- **Low-cost reversibility** due to the `OidcProvider` trait boundary: switching providers requires only a new adapter implementation and configuration change.
- **Security posture** benefits from Auth0's managed infrastructure (automatic patching, DDoS protection, compliance certifications).

### Negative

- **Vendor dependency** on Auth0/Okta for a critical authentication path. Mitigated by the provider-agnostic trait boundary that limits blast radius of a provider switch.
- **Limited customization** compared to self-hosted solutions (e.g., custom login flows, branding constraints on the free tier). Acceptable for MVP; can be revisited if user experience demands it.

### Risks

- **Pricing at scale:** Auth0's paid tiers (beyond 25k MAU) are priced per-user and can become expensive. If Haiker grows beyond the free tier, the team must evaluate whether Auth0's pricing remains competitive or whether migrating to a cheaper alternative (Cognito, self-hosted) is warranted. The `OidcProvider` trait boundary makes this migration feasible.
- **Auth0/Okta platform changes:** Okta's acquisition of Auth0 has introduced uncertainty about long-term product direction and pricing. Monitor announcements and maintain the ability to switch via the trait boundary.
- **Free tier limitations:** The free plan has restrictions (limited social connections, no custom domains, limited MFA options). If these become blockers, upgrading to a paid plan or switching providers will be necessary.
