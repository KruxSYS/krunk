-- Client addresses that aren't publicly routable are not the account's: they mean the request
-- reached the service on its own host network (a test, or a proxy that isn't configured to pass
-- the client on). The server no longer stores them (server/src/client_ip.rs, is_public), and
-- listings hide old rows that hold one. Clear the addresses themselves too, so a personal-data
-- download can't carry the deployment's internal network.
--
-- link_grants and pair_grants keep theirs: a grant is matched to the claiming device by address
-- (devices.rs), and they are short-lived; the export hides them where they aren't public.
UPDATE ip_events SET ip = NULL, country = NULL
 WHERE ip IS NOT NULL AND (
       ip << '10.0.0.0/8'::inet OR ip << '100.64.0.0/10'::inet OR ip << '172.16.0.0/12'::inet
    OR ip << '192.168.0.0/16'::inet OR ip << '127.0.0.0/8'::inet OR ip << '169.254.0.0/16'::inet
    OR ip << '0.0.0.0/8'::inet OR ip << '224.0.0.0/4'::inet OR ip << '240.0.0.0/4'::inet
    OR ip << 'fc00::/7'::inet OR ip << 'fe80::/10'::inet OR ip << '::1/128'::inet
    OR ip << '::/128'::inet OR ip << 'ff00::/8'::inet
 );

UPDATE web_sessions SET ip = NULL, country = NULL
 WHERE ip IS NOT NULL AND (
       ip << '10.0.0.0/8'::inet OR ip << '100.64.0.0/10'::inet OR ip << '172.16.0.0/12'::inet
    OR ip << '192.168.0.0/16'::inet OR ip << '127.0.0.0/8'::inet OR ip << '169.254.0.0/16'::inet
    OR ip << '0.0.0.0/8'::inet OR ip << '224.0.0.0/4'::inet OR ip << '240.0.0.0/4'::inet
    OR ip << 'fc00::/7'::inet OR ip << 'fe80::/10'::inet OR ip << '::1/128'::inet
    OR ip << '::/128'::inet OR ip << 'ff00::/8'::inet
 );

UPDATE devices SET last_ip = NULL, last_country = NULL
 WHERE last_ip IS NOT NULL AND (
       last_ip << '10.0.0.0/8'::inet OR last_ip << '100.64.0.0/10'::inet
    OR last_ip << '172.16.0.0/12'::inet OR last_ip << '192.168.0.0/16'::inet
    OR last_ip << '127.0.0.0/8'::inet OR last_ip << '169.254.0.0/16'::inet
    OR last_ip << '0.0.0.0/8'::inet OR last_ip << '224.0.0.0/4'::inet OR last_ip << '240.0.0.0/4'::inet
    OR last_ip << 'fc00::/7'::inet OR last_ip << 'fe80::/10'::inet OR last_ip << '::1/128'::inet
    OR last_ip << '::/128'::inet OR last_ip << 'ff00::/8'::inet
 );
