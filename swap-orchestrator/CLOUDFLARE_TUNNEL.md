To enable the Cloudflare Tunnel, set all four env vars before running ./orchestrator (all-or-none — partial config is a hard error):

export CLOUDFLARE_TUNNEL_TOKEN="<run token from Cloudflare Zero Trust dashboard>"
export CLOUDFLARE_TUNNEL_EXTERNAL_HOST="asb.example.com"   # public hostname peers dial
export CLOUDFLARE_TUNNEL_EXTERNAL_PORT=443                 # almost always 443 (wss)
export CLOUDFLARE_TUNNEL_INTERNAL_PORT=9940                # free port; must not be 9939/9839 or 9944

Then run ./orchestrator and docker compose up -d as normal. The orchestrator adds a cloudflared service, a /ws listener inside Docker, and advertises /dns4/<EXTERNAL_HOST>/tcp/<EXTERNAL_PORT>/wss to peers.
