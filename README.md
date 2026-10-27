# ForgeDesMondes

M'aide à produire et rouler mes sessions DND.

Le DM prépare ses campagnes (séances, entités, battlemaps, sons), puis fait
rouler la séance avec la **DM view** et la **player view**, dans deux onglets
synchronisés.

## Ce qui tourne

| Morceau | Quoi | Port par défaut |
|---|---|---|
| **Serveur** (`crates/server`, Rust) | le protocole de l'application (`/capabilities/<id>`, `/commands`), PostgreSQL derrière | `8790` |
| **Shell** (`apps/shell`, Node) | l'hôte du frontend : sert les écrans, le design system, les sons, et relaie le protocole au serveur | `8791` |
| **PostgreSQL** | la base de l'application | `55432` (dans cette doc) |
| **Maquette** (`mock/`) | HTML/JS statique, sans serveur ni base : la maquette de la preuve de concept | — |

Aujourd'hui les ports vers l'extérieur sont des **fakes** (`PORTS=fake`) : rien
ne sort de la machine et aucune clé n'est nécessaire. L'adaptateur `llm` réel
(Claude Code, `claude -p --model claude-haiku-5-5 --effort medium`) existe mais
**n'est pas encore branché** au serveur (voir `docs/toBeCheck/llm-claude-code-adapter.md`).

## Prérequis

- **Docker** (OrbStack ou Docker Desktop) pour PostgreSQL
- **Rust 1.99.1** : `rustup` lit `rust-toolchain.toml` et l'installe seul
- **Node ≥ 22** et npm
- `curl` pour vérifier

## Lancer l'application

Depuis la racine du dépôt.

### 1. PostgreSQL

```bash
docker run -d --name fdm-postgres \
  -e POSTGRES_USER=fdm -e POSTGRES_PASSWORD=fdm -e POSTGRES_DB=fdm \
  -p 127.0.0.1:55432:5432 \
  -v fdm-pgdata:/var/lib/postgresql/data \
  postgres:16
```

Attendre qu'elle réponde :

```bash
docker exec fdm-postgres pg_isready -U fdm -d fdm
```

### 2. Les migrations

À rejouer après chaque `git pull` (elles sont idempotentes) :

```bash
for f in $(git ls-files -- '*migrations/*.sql' | LC_ALL=C sort); do
  docker exec -i fdm-postgres psql -U fdm -d fdm -v ON_ERROR_STOP=1 -q < "$f" >/dev/null
done
```

Des `NOTICE … does not exist, skipping` à la première exécution sont normaux.

### 3. Construire

```bash
cargo build --release -p server
(cd apps/shell && npm install && npm run build)
```

### 4. Démarrer le serveur

```bash
DATABASE_URL=postgres://fdm:fdm@127.0.0.1:55432/fdm \
PORT=8790 PORTS=fake FIXTURES_DIR="$PWD/mock/fixtures" \
  ./target/release/server
```

- `DATABASE_URL` est obligatoire.
- `FIXTURES_DIR` charge les données de démonstration (trois campagnes, une
  séance en cours…) dans la base au démarrage. Sans lui, la base reste vide.
- Il écoute sur toutes les interfaces ; `curl http://127.0.0.1:8790/health`
  répond `{"status":"ok"}`.

### 5. Démarrer le shell (dans un autre terminal)

```bash
cd apps/shell
PORT=8791 SERVER_URL=http://127.0.0.1:8790 npm run serve
```

### 6. Ouvrir

<http://127.0.0.1:8791/> liste les écrans. Les principaux :

| Écran | URL |
|---|---|
| Accueil des campagnes | `/screens/campagnes` |
| Campagne | `/screens/campagne` |
| Fiche d'entité | `/screens/fiche-entite` |
| Préparation de séance | `/screens/preparation-seance` |
| Médias et réglage de battlemap | `/screens/medias` |
| Soundbox | `/screens/soundbox` |
| Paramètres IA | `/screens/parametres-ia` |
| **DM view** | `/screens/dm-view` |
| **Player view** | `/screens/player-view` |

Pour la séance : ouvrir la DM view et la player view **dans deux onglets** du
même navigateur ; la player view montre ce que le DM lui envoie
(« Envoyer aux joueurs » sur la battlemap).

## Arrêter

```bash
# Ctrl-C dans chaque terminal, ou :
pkill -f target/release/server        # le serveur
docker stop fdm-postgres              # la base (les données restent dans le volume)
docker start fdm-postgres             # pour la reprendre
```

Repartir de zéro : `docker rm -f fdm-postgres && docker volume rm fdm-pgdata`.

## Autres façons de lancer

- **La maquette seule**, sans serveur ni base : servir le dossier `mock/` tel
  quel, par exemple `python3 -m http.server -d mock 8000`, puis ouvrir
  <http://127.0.0.1:8000/> (voir `mock/README.md`).
- **Le runtime des règles** (base jetable, supprimée à l'arrêt ; sert aussi la
  CI) : `bash scripts/rules-runtime.sh serve` avec `DATABASE_URL` pointant sur un
  PostgreSQL superutilisateur ; il construit, migre, charge les fixtures et
  démarre serveur et shell. Il affiche `RULES_APP_URL` ; `bash
  scripts/rules-runtime.sh stop` nettoie.

## Vérifier

```bash
cargo test --workspace            # Rust
(cd apps/shell && npm test)       # le shell
bash scripts/gates-local.sh       # les mêmes contrôles que la CI
```

## Où lire la suite

- `CLAUDE.md` : les règles que tout changement respecte.
- `docs/ARCHITECTURE.md`, `docs/DATA-INTEGRITY.md` : l'architecture et les
  garanties sur les données.
- `rules.json`, `tests/rules/` : les règles du produit et leurs tests.
- `ports.json`, `secrets/README.md` : les ports vers l'extérieur et leurs secrets.
