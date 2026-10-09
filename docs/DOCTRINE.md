# Agent-First Game Engine: Doctrine

## How to read this

Read this as a trusted, capable manager building an engine that you and others will use. We don't need to tell you to lock the door at night, keep company secrets off public message boards, avoid burning the place down, or get approval before spending money. Use your judgment. When you think going against this doctrine is best, escalate and explain your reasoning. Rules, principles need to be flexible and we have a process to allow for that.

## North Star

Build a game engine designed from the ground up for AI coding agents ("agents"). Every layer is built so agents can develop, test, inspect, and debug it directly. We deliberately leave behind human-centric development tooling and any development method agents cannot use well. The result: games built faster, more consistently, and at higher quality than with general-purpose engines retrofitted for agents.

Analogy: build a fully autonomous fighter jet with no cockpit. Removing the pilot removes the constraints a human imposes on the design. Humans give direction remotely; agents fly the plane.

Optimize for the best expected overall output, not flawless software. Solve problems when they actually surface, not in anticipation.

## Principles

1. Agent-readable
   Code, data, and project structure are optimized for agent comprehension over conventions that serve only humans.

2. Agent-operable
   Every capability is exposed through machine interfaces. Nothing requires a GUI unless no other option.

3. Verifiable without a display
   Everything can be run, tested, inspected, and verified without a screen or a human watching.

4. Agent-accessible assets
   Source assets are created in this order of preference:
   1. Procedural: code and data that generate the asset.
   2. Text/data: formats agents read and edit directly.
   3. Binary: only with human approval. Approved: fonts (WOFF2, TTF, OTF).

   Rationale: agents work best with code and data, and are improving at that faster than legacy asset tools are improving for agents.

5. Reproducible on the development platform
   On the development platform, identical inputs and seeds produce identical gameplay state, and any gameplay state can be captured, restored, and replayed.

6. WebGPU only
   WebGPU is the only renderer, in a browser or through a native implementation (ie Vulkan). Gameplay runs only on the CPU. Game UI in HTML/CSS is allowed.

7. Common ground
   Everything agents work with when building games uses widely used languages, libraries, systems, and patterns. Favor flexibility, ease of use, and established approaches over cutting-edge performance and capabilities.

   Analogy: build the Sherman, not the Tiger.

   Rationale: lean into what agents already do well rather than trying to change it.

8. Quality under the hood
   Engine internals, meaning code whose API does not appear in game code, use the highest-quality approach their builder can execute well, however complex. Reuse what others have already built well, and invest heavily in what is ours. That complexity stays behind the API: agents need to use the engine well, not read or rebuild it, even when something goes wrong.

   Rationale: the engine is built once and used many times, so better internals raise the quality of everything built on it.

9. Mastery over novelty
   Use the newest version that is at least 12 months old, so agents know it deeply. A newer version that is backward compatible with a qualifying version, or otherwise works the same way, is adopted immediately, since agents' existing knowledge still applies. Within engine internals covered by principle 8, the builder may use newer versions when they raise quality and it can use them well. Upgrade as newer versions qualify.

10. Discovery first, hardening later
    Develop and test against a single target platform: the one the development environment runs on. Cross-platform compatibility and hardening happen when a game goes to production.

    Rationale: most of the work is finding something worth shipping.

## Escalation

When an action requires human approval or a principle cannot be satisfied, escalate and wait. If there is no response within 15 minutes, commit current work, make the call, and continue. For the rest of that run, or until a human responds, report further conflicts without stopping. Record every escalation, every later conflict, and every call made without a response, so each can be found and reviewed.

Rationale: progress never stalls indefinitely. The worst case is a review, a change, or a rollback.
