# Test fixtures

| File | What | Source |
| --- | --- | --- |
| `pgs_sub.sup` | Blu-ray PGS (`hdmv_pgs_subtitle`) stream, 1920×1080, events at 0.0–0.4 s, 49 382 bytes | FFmpeg FATE test suite, <https://fate-suite.ffmpeg.org/sub/pgs_sub.sup> (SHA-256 `ce6d8ed89cf557e34b90d49c5ce955731935c485b93892f69ab276cfe0c36c27`) |

FFmpeg cannot encode PGS, so this one sample is committed; the FATE suite is
FFmpeg's public regression corpus, distributed for testing. All other test media
is generated with `ffmpeg` while the tests run and never committed.
