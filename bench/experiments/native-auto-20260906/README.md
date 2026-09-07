# Native versus automatic improvement experiment

Status: all 918 frozen baseline calls and all 334 exact-content audit labels are complete.
The improvement loop remains in progress; no final winner or technical plateau is established.

The primary useful-delivery count is auto 142 versus native 140. If useful records on responses
marked blocked are also counted, auto has 144 versus native 149. These small differences, broad
round-to-round ranges and shared-ledger carryover do not establish a content winner. Auto spent
less total fetch time (1,717 seconds versus 2,986), including unsuccessful calls and local
refusals; this is workload efficiency, not a per-page speed comparison at equal admission.

Candidate 1 is now under [complete confirmation](../native-auto-candidate1-20260907/README.md).

See [the prespecified protocol](protocol.md). The run covers 918 calls, followed by an
evidence-driven improvement loop and a complete confirmation comparison for retained changes.
This directory is separate from the earlier automatic-only snapshot.

## Journal

- Measurement preparation: confirmed that setting native identity alone still permits HTTP.
  Added a dedicated measurement entry point that explicitly uses native `warm` versus default
  `auto`, with equal deadlines and extraction. Product behavior has not changed.
- TDD: the missing native-control/continuation helpers failed compilation before implementation.
  The first continuation fixture failed because raw markup URLs are not cacheable; replaced it
  with a loopback HTTP fixture that closes after one transport. Both tests now pass, including
  retrieval of the document tail through cache-only continuations.
- Controller TDD: three tests failed to import the unimplemented controller, then passed after
  implementation. They cover all 918 scheduled visits, leading-arm alternation, incomplete
  result rejection and accounting for failed-call time.
- Baseline build completed successfully. The benchmark test binary passed 41 tests with four
  corpus-dependent tests ignored; clippy for all benchmark targets passed with warnings denied.
  Three offline reporting tests also passed. The full product QC from the preceding session
  remains separately recorded; no new full-QC result is claimed here yet.
- Frozen baseline executable SHA-256:
  `6f72215a78789362026edf836e1aeaebcc6e5f4bfa5338f063e73ae6edfb77e9`.
  The revision is e60e10b plus the source modifications listed by hash in the manifest.
- Live aggregate output is in [results.md](results.md) and [summary.json](summary.json).
  Both state whether all 918 calls are complete. [Investigation notes](investigation.md)
  distinguish observed deficits from untested improvement hypotheses.
- Operator-reported power loss: 30 completed calls were recovered and all raw hashes verified.
  One partially executed block lost its response output; its log and corrupt progress file
  are archived. The block is repeated with retained profiles and recovered traffic accounting.
  Two extra admissions survived in SQLite; unflushed reputation spend was conservatively
  restored. See [the recovery record](interruption-recovery.json). This deviation will remain
  visible in final results, including a sensitivity comparison excluding the affected target.
