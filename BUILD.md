# BUILD plato-e2e-pipeline — End-to-End DCS → Belief → Deploy

## What to Build
The first end-to-end integration where:
1. DCS runs a 7-phase cycle (divide → assign → compute → verify → synthesize → validate → integrate)
2. DCS output produces tiles with quality scores
3. Tiles feed into unified-belief scoring (confidence, trust, relevance)
4. Belief scores feed into deploy-policy tier classification (Live/Monitored/HumanGated)
5. The pipeline returns a PipelineResult with everything wired together

This proves the entire stack works as one system, not isolated crates.

## Dependencies
Clone these repos to inspect their APIs:
- gh repo clone SuperInstance/plato-dcs -- --depth 1 /tmp/deps-e2e/plato-dcs
- gh repo clone SuperInstance/plato-unified-belief -- --depth 1 /tmp/deps-e2e/plato-unified-belief
- gh repo clone SuperInstance/plato-deploy-policy -- --depth 1 /tmp/deps-e2e/plato-deploy-policy
- gh repo clone SuperInstance/plato-tile-spec -- --depth 1 /tmp/deps-e2e/plato-tile-spec

## Design
- E2EPipeline struct with run() method
- Takes: problem description, agent pool, tile set
- Returns: E2EResult with DCS output, belief scores, deploy decisions, stats
- 3 integration test scenarios:
  1. Happy path: 4 agents, clean problem → all tiles deploy Live
  2. Mixed: 4 agents, some fail verify → mixed tiers
  3. Failure: all agents fail → all HumanGated

## Key Assertion
The DCS specialist ratio (5.88×) must be reflected in belief scores:
specialist-assigned tiles should have higher confidence than generalist-only tiles.

## Quality
- Clean API that other fleet agents can call
- Comprehensive documentation explaining the data flow
- Stats: total tiles processed, tier distribution, average belief, processing time

BUILD IT NOW. Write Cargo.toml with git deps, src/lib.rs with E2EPipeline, comprehensive tests.
If git deps fail on cargo 1.75, inline the needed APIs with attribution.
Zero external deps beyond fleet crates. Push to GitHub when tests pass.
