# Preconnect netlog proof (app260n, engine 33bbbe35, 2026-11-21, clean-exit rerun)

Page `scripts/vm-www/8092/preconnect.html`: preconnect + dns-prefetch to example.org (normal), stats.g.doubleclick.net / ad.doubleclick.net (blocked), rel=prerender to example.net.

- **on** (shipped): example.org DNS job; doubleclick.net nothing; example.net nothing (NoStatePrefetch held back).
- **off** (control, `KRUNK_PERF_OFF=preconnect-block`): doubleclick.net DNS + socket (group stats.g.doubleclick.net); example.org DNS + socket.
- **private**: nothing for any of them.
- Open: in this single 'on' run example.org got its DNS job but no socket; example.org is in none of the 8 default lists (checked) and the set builder needs a dotted host, so the patch can't drop it. Rechecked in the final smoke.
- First attempt (force-killed Krunk) left the netlogs cut off/empty: the script now waits for a clean exit.

## on
```
example.org: contacted
  DNS_TRANSACTION 2
  DNS_TRANSACTION_QUERY 2
  HOST_RESOLVER_MANAGER_JOB 1
  HOST_RESOLVER_MANAGER_REQUEST 1
doubleclick.net: none
example.net: none
```

## off
```
example.org: contacted
  CERT_VERIFIER_JOB 1
  CERT_VERIFY_PROC 1
  CERT_VERIFY_PROC_PATH_BUILDER_DEBUG 4
  DNS_TRANSACTION 2
  DNS_TRANSACTION_QUERY 2
  HOST_RESOLVER_DNS_TASK 1
  HOST_RESOLVER_DNS_TASK_EXTRACTION_RESULTS 1
  HOST_RESOLVER_MANAGER_CACHE_HIT 6
  HOST_RESOLVER_MANAGER_JOB 1
  HOST_RESOLVER_MANAGER_REQUEST 3
  HTTP_STREAM_JOB_CONTROLLER 2
  SOCKET_POOL_CONNECT_JOB_CREATED 1
  TCP_CLIENT_SOCKET_POOL_REQUESTED_SOCKETS 2
doubleclick.net: contacted
  CERT_VERIFIER_JOB 1
  CERT_VERIFY_PROC 1
  CERT_VERIFY_PROC_PATH_BUILDER_DEBUG 2
  DNS_TRANSACTION 4
  DNS_TRANSACTION_QUERY 4
  HOST_RESOLVER_DNS_TASK 2
  HOST_RESOLVER_DNS_TASK_EXTRACTION_RESULTS 4
  HOST_RESOLVER_MANAGER_CACHE_HIT 4
  HOST_RESOLVER_MANAGER_JOB 2
  HOST_RESOLVER_MANAGER_REQUEST 3
  HTTP_STREAM_JOB_CONTROLLER 2
  SOCKET_POOL_CONNECT_JOB_CREATED 1
  TCP_CLIENT_SOCKET_POOL_REQUESTED_SOCKETS 2
example.net: none
```

## private
```
example.org: none
doubleclick.net: none
example.net: none
```
