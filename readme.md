# Cadet

Jetstream listener and ingester for teal.fm

### i thought everything was named  after blues?????
- cadet blue. [its real!](https://en.wikipedia.org/wiki/Cadet_grey#Cadet_blue)


# cadet development setup
1. Follow the setup on the main repo for [teal.fm](https://github.com/teal-fm/teal)
2. Copy [.env.template](.env.template) to [.env](.env) at the root of the repo
3. Fill in the `.env` with your postgres db url. This is the same DB as you use to setup aqua(teal.fm's appview). Make sure to run the migrations from the main repo first.
4. run the JetStream lister from the main repo with `cargo run --bin cadet`
(optional). Can run [satellite](./satellite/readme.md) for some stats with `cargo run --bin satellite`

# running with docker
Ideally if you are just needing to run to cadet locally with aqua you will most likely just need this

1. Make sure docker and docker compose is installed and setup
2. Follow steps 1-3 of [cadet development](#cadet-development-setup) to make sure your DB is setup and migrations has been ran
3. run `docker compose up -d` to run both [cadet](./cadet/) [satellite](./satellite/).