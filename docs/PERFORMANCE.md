# Performance record (M7)

Fill in after running `docs/manual-tests/M7.md` on the target PC (Windows 11 laptop, 50 Mbps).

| NFR | Target | Measured | Date | Notes |
|---|---|---|---|---|
| NFR-01 chord to recording start | ≤ 150 ms | | | |
| NFR-02 dictation release to text (p90, ≤ 10 s clips) | ≤ 4 s | | | |
| NFR-03 Circle to Ask release to answer (p90) | ≤ 6 s | | | |
| NFR-04 hook callback | ≤ 5 ms | by design: state update + channel send only | | |
| NFR-05 idle CPU / RAM incl. WebView2 | < 1% / < 200 MB | | | |
| NFR-06 cold start to hotkeys | ≤ 3 s | | | |
| NFR-07 job start after due time | ≤ 5 s | | | |
| NFR-08 emergency stop | ≤ 1 s | < 1 s in the mock-server cancellation test | 2026-10-13 | `cancellation_stops_in_flight_request` |
| Soak: 2 h, memory growth | < 50 MB, no hook loss | | | |
