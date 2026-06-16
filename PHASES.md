# SafeSelect — Fases de Implementación

Instrucciones detalladas para cada fase. Cada fase produce un binario funcional y verificable.

---

## Fase 1: Esqueleto + CLI (días 1-2)

### Objetivo
Binary que parsea argumentos, carga configuración TOML y la valida. Sin MCP, sin sidecar.

### Dependencias iniciales Cargo.toml

```toml
[package]
name = "safeselect"
version = "0.1.0"
edition = "2021"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
dirs = "6"
```

### Archivos a crear

```
src/main.rs           # fn main, init tracing, dispatch clap
src/cli.rs            # Cli struct con todos los subcomandos
src/config/mod.rs     # ConfigLoader, validate_all(), load_project(), load_environment(), load_driver()
src/config/project.rs # ProjectConfig (deser desde project.toml)
src/config/environment.rs # EnvironmentConfig
src/config/driver.rs  # DriverConfig
src/error.rs          # SafeselectError enum (thiserror)
```

### Criterio de éxito

```bash
cargo run -- config validate
# → Error: no project specified

cargo run -- config validate --project ecommerce
# → Error: project directory not found

# Tras crear ~/.config/safeselect/projects/ecommerce/{project.toml,environments/testing.toml}
cargo run -- config validate --project ecommerce --environment testing
# → Config valid: ecommerce/testing
```

### Comandos CLI implementados
- `safeselect serve --project <x> --environment <y>` — placeholder que imprime "Not implemented yet"
- `safeselect config validate` — carga y valida
- `safeselect config show` — imprimer config resuelta (sin secretos)
- `safeselect driver list` — lista drivers en `~/.config/safeselect/drivers/*.toml`
- `safeselect driver add` — escribe archivo driver.toml con SHA-256 calculado
- `safeselect driver download --vendor postgresql` — placeholder
- `safeselect agent detect` — placeholder
- `safeselect check` — placeholder
- `safeselect import dbeaver <zip>` — placeholder

### Verificación
```bash
cargo build
cargo test
cargo clippy
```

---

## Fase 2: MCP server (días 3-4)

### Objetivo
SafeSelect inicia un MCP server sobre stdio. Agentes pueden conectarse y llamar a `select`, `list_tables` y `explain`. Sin base de datos real — el sidecar se mockea.

### Nueva dependencia
```toml
rmp-via = "0.1"  # MCP SDK Rust (o alternativamente json! manual + serde)
```

Si `rmp-via` no está maduro, usar JSON-RPC manual:

```rust
// Protocolo manual sobre stdio
// stdin → serde_json::Deserializer::from_reader(stdin)
// stdout → serde_json::to_writer(stdout, response)
```

### Archivos a crear/modificar

```
src/main.rs           # serve → inicia McpServer
src/mcp.rs            # McpServer: initialize, list_tools, call_tool
src/mcp/tools.rs      # handlers: select, list_tables, explain
```

### Protocolo MCP (manual)

```rust
// Initialize
← {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"0.1.0","capabilities":{},"clientInfo":{"name":"opencode","version":"1.0"}}}
→ {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"0.1.0","capabilities":{"tools":{}},"serverInfo":{"name":"safeselect","version":"0.1.0"}}}

// ListTools
← {"jsonrpc":"2.0","id":2,"method":"tools/list"}
→ {"jsonrpc":"2.0","id":2,"result":{"tools":[
  {"name":"select","description":"Execute a SELECT query","inputSchema":{"type":"object","properties":{"sql":{"type":"string"}},"required":["sql"]}},
  {"name":"list_tables","description":"List tables in a schema","inputSchema":{"type":"object","properties":{"schema":{"type":"string"}}}},
  {"name":"explain","description":"Show query execution plan","inputSchema":{"type":"object","properties":{"sql":{"type":"string"}},"required":["sql"]}}
]}}

// CallTool
← {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_tables","arguments":{"schema":"public"}}}
→ {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"[{\"schema\":\"public\",\"name\":\"users\"}]"}]}}
```

### Criterio de éxito
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"0.1.0","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | cargo run -- serve --project ecommerce --environment testing
# → Responde con initialize + tools/list (mock)

cargo test
```

---

## Fase 3: Sidecar Java (días 5-7)

### Objetivo
Sidecar Java funcional que recibe JSON por stdin, ejecuta JDBC y responde por stdout. Rust lo lanza, monitorea y mata.

### Sidecar Java

```
sidecar/
├── pom.xml
└── src/main/java/com/safeselect/
    ├── Main.java          # stdin/out loop con ObjectMapper
    ├── Protocol.java      # Request/Response records
    ├── JdbcExecutor.java  # DriverManager.getConnection, execute, close
    └── Sidecar.java       # Lifecycle: init, health, shutdown
```

### Protocolo JSON-lines

```
Request → {"id":1,"method":"execute","params":{"sql":"SELECT 1"}}
Response ← {"id":1,"ok":{"columns":["?column?"],"rows":[[1]]}}

Request → {"id":2,"method":"ping"}
Response ← {"id":2,"ok":"pong"}

Request → {"id":3,"method":"execute","params":{"sql":"INVALID"}}
Response ← {"id":3,"error":{"code":"SQL_ERROR","message":"syntax error at ..."}}

Request → {"id":4,"method":"shutdown"}
Response ← {"id":4,"ok":"bye"}
```

### Build del sidecar

```bash
cd sidecar && mvn package -DskipTests
# → sidecar/target/safeselect-sidecar.jar
```

En el build de Rust, se embeble:

```rust
// build.rs
fn main() {
    println!("cargo:rerun-if-changed=sidecar/target/safeselect-sidecar.jar");
}

// src/sidecar/mod.rs (runtime)
const SIDECAR_JAR: &[u8] = include_bytes!("../../sidecar/target/safeselect-sidecar.jar");

fn ensure_sidecar() -> PathBuf {
    let path = dirs::data_dir()
        .unwrap()
        .join("safeselect")
        .join("sidecar")
        .join("safeselect-sidecar.jar");
    if !path.exists() {
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, SIDECAR_JAR)?;
    }
    // Validate JAR integrity (SHA-256)
    path
}

fn start_sidecar(jar: &Path) -> Result<Child> {
    Command::new("java")
        .arg("-jar")
        .arg(jar)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
}
```

### Integración Rust

```rust
// src/sidecar/mod.rs
pub struct SidecarProcess {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl SidecarProcess {
    pub fn start(config: &EnvironmentConfig) -> Result<Self>;
    pub fn execute(&mut self, sql: &str) -> Result<QueryResult>;
    pub fn ping(&mut self) -> Result<()>;
    pub fn shutdown(mut self) -> Result<()>;
}
```

### Criterio de éxito
```bash
# Primero registrar driver PostgreSQL
cargo run -- driver add --vendor postgresql --path /path/to/postgresql.jar

# Luego servir
cargo run -- serve --project ecommerce --environment testing
# → Arranca sidecar, hace ping, escucha MCP en stdio

# En otro terminal:
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_tables","arguments":{}}}' | cargo run -- serve --project ecommerce --environment testing
```

---

## Fase 4: Seguridad (días 8-10)

### Objetivo
Pipeline de seguridad completo: parseo AST, validación de políticas, fail-closed. Audit log persistente.

### Nueva dependencia
```toml
pg_query = "0.6"  # Bindings a libpg_query de PostgreSQL
```

### Pipeline

```rust
// src/security/mod.rs
pub fn validate_and_execute(
    sql: &str,
    policy: &ProjectConfig,
    sidecar: &mut SidecarProcess,
) -> Result<QueryResult> {
    let audit = AuditLog::open(&policy.audit)?;

    // 1. Parse AST
    let ast = Parser::parse(sql).map_err(|e| {
        audit.record("PARSE_ERROR", &sql, "reject");
        SecurityError::ParseError(e)
    })?;

    // 2. Validate
    let validator = PolicyValidator::new(&policy.security);
    validator.validate(&ast)?;  // lanza SecurityError si falla

    // 3. Execute via sidecar
    let result = sidecar.execute(sql)?;

    // 4. Check limits
    let limits = LimitChecker::new(&policy.limits);
    limits.check(&result)?;

    audit.record("PASS", &sql, "allow");
    Ok(result)
}
```

### Validaciones concretas

| Regla | Implementación |
|---|---|
| `require_single_statement` | AST.stmts.len() == 1 |
| `read_only` | AST contiene solo SelectStmt o ExplainStmt |
| `allowed_schemas` | AST.relations().all(|r| schemas.contains(r.schema)) |
| `denied_relations` | AST.relations().none(|r| denied.contains(r.full_name())) |
| `allow_volatile_functions` | AST.functions().all(|f| !f.is_volatile()) |
| `statement_timeout_ms` | `SET statement_timeout = value` antes de ejecutar |

### Fail-closed en Rust

```rust
#[derive(Debug, Snafu)]
enum SecurityError {
    ParseError { source: pg_query::Error },
    MultipleStatements { count: usize },
    NotReadOnly { stmt_type: String },
    SchemaNotAllowed { schema: String },
    RelationDenied { relation: String },
    VolatileFunction { function: String },
    LimitExceeded { kind: String, actual: usize, max: usize },
}

// Cuando se lanza SecurityError:
// 1. Close JDBC connection (sidecar.shutdown)
// 2. Kill sidecar process
// 3. Log audit entry
// 4. Exit process with non-zero
```

### Audit log

```rust
// src/audit.rs
pub struct AuditLog {
    writer: BufWriter<File>,
}

#[derive(Serialize)]
pub struct AuditEntry {
    timestamp: String,       // ISO 8601
    mcp_client: Option<String>,  // del initialize
    project: String,
    environment: String,
    category: String,        // PASS | PARSE_ERROR | POLICY_REJECT | LIMIT_EXCEEDED | INCIDENT
    decision: String,        // allow | reject
    query_hash: String,      // SHA-256 del SQL original
}
```

### Criterio de éxito
```bash
# Query permitida
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"select","arguments":{"sql":"SELECT * FROM public.users LIMIT 1"}}}'

# Query denegada
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"select","arguments":{"sql":"DELETE FROM public.users"}}}'
# → MCP error + proceso muere

# Verificar audit log
cat ~/.local/state/safeselect/audit/ecommerce/*.jsonl
```

---

## Fase 5: Secretos + Drivers (días 11-12)

### Objetivo
Resolución de secretos vía macOS Keychain y env vars. Registro y validación de drivers JDBC.

### macOS Keychain

```rust
// Sin crate externo — usar Command::new("security")
pub fn resolve_macos_keychain(service: &str, account: &str) -> Result<String> {
    let output = Command::new("security")
        .args(["find-generic-password", "-a", account, "-s", service, "-w"])
        .output()
        .context(SecretError::CommandFailed)?;

    if !output.status.success() {
        return Err(SecretError::NotFound);
    }

    Ok(String::from_utf8(output.stdout)
        .context(SecretError::InvalidEncoding)?
        .trim()
        .to_owned())
}
```

### Caching de secretos
- El secreto se resuelve una vez al iniciar `serve`
- Se mantiene en memoria durante la vida del proceso
- Nunca se escribe a disco

### Validación de drivers

```rust
pub fn validate_driver(config: &DriverConfig) -> Result<()> {
    // 1. Existe el archivo
    let path = Path::new(&config.path);
    ensure!(path.exists(), DriverError::NotFound);

    // 2. Permisos seguros (solo owner)
    let metadata = path.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        ensure!(mode & 0o007 == 0, DriverError::InsecurePermissions);
    }

    // 3. SHA-256 checksum
    let actual = sha256_file(path)?;
    ensure!(actual == config.sha256, DriverError::ChecksumMismatch);

    Ok(())
}
```

### Driver download

```rust
pub fn download_official(vendor: &str) -> Result<DriverConfig> {
    match vendor {
        "postgresql" => {
            // https://jdbc.postgresql.org/download/postgresql-42.7.4.jar
            let url = "https://jdbc.postgresql.org/download/postgresql-42.7.4.jar";
            let jar_path = driver_dir().join("postgresql.jar");
            download_file(url, &jar_path)?;
            let sha = sha256_file(&jar_path)?;
            write_driver_toml("postgresql", &jar_path, "org.postgresql.Driver", &sha)
        }
        _ => bail!("Unknown vendor: {vendor}. Use `safeselect driver add` for custom drivers."),
    }
}
```

### Criterio de éxito
```bash
# Keychain
security add-generic-password -a "ecommerce/testing" -s "safeselect" -w "secret123"
cargo run -- check --project ecommerce --environment testing
# → Conecta a base de datos real

# Driver download
cargo run -- driver download --vendor postgresql
# → Descarga + registra automáticamente
```

---

## Fase 6: Agentes (días 13-15)

### Objetivo
Instalación/desinstalación automática en cada cliente MCP. Skill manifest para OpenCode. Import DBeaver.

### Estructura de agentes

```rust
// src/agents.rs
pub trait McpClient {
    fn name(&self) -> &str;
    fn config_path(&self) -> Result<PathBuf>;
    fn detect(&self) -> Result<bool>;
    fn read_config(&self) -> Result<String>;
    fn write_config(&self, content: &str) -> Result<()>;
    fn backup_path(&self) -> PathBuf;
}

// Implementaciones para cada cliente
pub struct OpenCodeClient;
pub struct CodexClient;
pub struct ClaudeCodeClient;
pub struct CopilotClient;
pub struct CursorClient;
pub struct WindsurfClient;
pub struct GeminiCliClient;
```

### Instalación atómica

```rust
pub fn install(client: &dyn McpClient, args: &InstallArgs) -> Result<()> {
    // 1. Detectar cliente
    let installed = client.detect()?;
    bail_if!(!installed, "{} not found", client.name());

    // 2. Leer config
    let content = client.read_config()?;

    // 3. Validar formato
    let mut config = parse_config(&content)?;

    // 4. Verificar que no exista una entrada con el mismo nombre
    bail_if!(config.has_entry(&args.name), "entry '{}' already exists", args.name);

    // 5. Preparar nueva entrada
    let entry = McpEntry {
        name: args.name.clone(),
        project: args.project.clone(),
        environment: args.environment.clone(),
    };
    config.add_entry(entry);

    // 6. Mostrar diff y confirmar
    let new_content = config.to_string();
    show_diff(&content, &new_content);
    confirm()?;

    // 7. Backup
    fs::copy(client.config_path(), client.backup_path())?;

    // 8. Escribir atómicamente
    atomic_write(client.config_path(), &new_content)?;

    // 9. Releer y validar
    let re_read = client.read_config()?;
    bail_if!(re_read != new_content, "write verification failed");

    Ok(())
}
```

### Reglas de seguridad en instalación
- Rechazar configs con enlaces simbólicos
- Rechazar permisos inseguros (group/world-writable)
- Rechazar formatos desconocidos o ambiguos
- Rechazar campos extraños
- Backup antes de escribir

### OpenCode skill manifest

```yaml
# skills/safeselect.md
---
name: safeselect
description: SafeSelect MCP SQL Fail-Closed — secure database access for AI agents
tools:
  - select
  - list_tables
  - explain
setup: |
  safeselect agent install opencode --project <project> --environment <environment> --name <name>
commands:
  - safeselect serve --project <name> --environment <env>
  - safeselect config validate --project <name> --environment <env>
  - safeselect driver add --vendor postgresql --path <jar> --class <class>
```

### DBeaver import

```rust
pub fn import_dbeaver(zip_path: &Path) -> Result<()> {
    let archive = File::open(zip_path)?;
    let mut zip = ZipArchive::new(archive)?;

    // Leer .dbeaver/data-sources.json
    // Extraer conexiones: host, port, database, driver name
    // NO importar: credentials, drivers, scripts, commands
    // Generar project.toml + environments/<name>.toml

    // Mostrar resumen de lo que se va a crear
    // Confirmar antes de escribir
    for conn in connections {
        write_environment(&conn)?;
    }

    Ok(())
}
```

### Criterio de éxito
```bash
cargo run -- agent detect
# → Found: opencode (v0.x), cursor (v0.x)

cargo run -- agent install opencode --project ecommerce --environment testing --name "ecommerce-testing"
# → Diff + confirm + backup + write + verify

cargo run -- agent status
# → ecommerce-testing → opencode → ecommerce/testing ✓
```

---

## Fase 7: Distribución (días 16-17)

### Objetivo
Builds automáticos, Homebrew, asdf, release pipeline.

### GitHub Actions workflow

```yaml
# .github/workflows/release.yml
on:
  push:
    tags: ["v*"]

jobs:
  build:
    strategy:
      matrix:
        target: [x86_64-apple-darwin, aarch64-apple-darwin, x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu]
    steps:
      - uses: actions/checkout@v4
      - uses: actions-rust-lang/setup-rust-toolchain@v1
      - run: cargo build --release --target ${{ matrix.target }}
      - run: cargo test
      - run: gzip -c target/${{ matrix.target }}/release/safeselect > safeselect-${{ matrix.target }}.gz
      - uses: softprops/action-gh-release@v1
        with:
          files: safeselect-*.gz
```

### Homebrew formula

```ruby
# Formula/safeselect.rb (repo tap separado o en anomalyco/homebrew-tap)
class Safeselect < Formula
  desc "MCP SQL Fail-Closed for AI Agents"
  homepage "https://github.com/anomalyco/safeselect"
  version "0.1.0"

  if OS.mac?
    if Hardware::CPU.arm?
      url "https://github.com/anomalyco/safeselect/releases/download/v0.1.0/safeselect-aarch64-apple-darwin.gz"
      sha256 "..."
    else
      url "https://github.com/anomalyco/safeselect/releases/download/v0.1.0/safeselect-x86_64-apple-darwin.gz"
      sha256 "..."
    end
  else
    # Linux
  end

  depends_on "openjdk@17"

  def install
    bin.install "safeselect"
  end

  test do
    assert_match "safeselect 0.1.0", shell_output("#{bin}/safeselect --version")
  end
end
```

### asdf plugin

```
# Repositorio: github.com/anomalyco/asdf-safeselect
# bin/list-all → consulta GitHub Releases, lista tags
# bin/download → descarga el binary para la plataforma actual
# bin/install  → extrae y enlaza
```

### Criterio de éxito
```bash
brew install anomalyco/tap/safeselect
safeselect --version
# → safeselect 0.1.0

asdf plugin add safeselect https://github.com/anomalyco/asdf-safeselect
asdf install safeselect 0.1.0
safeselect --version
# → safeselect 0.1.0
```

---

## Dependencias Rust (resumen)

| Crate | Fase | Propósito |
|---|---|---|
| `clap` (derive) | 1 | CLI argument parser |
| `serde` + `serde_json` | 1 | Serialización/deserialización |
| `toml` | 1 | Config TOML parser |
| `thiserror` | 1 | Error types |
| `tracing` + `tracing-subscriber` | 1 | Logging |
| `dirs` | 1 | XDG directories |
| `sha2` | 1 | SHA-256 para drivers y audit |
| `rmp-via` (o jsonrpc manual) | 2 | MCP protocol |
| `pg_query` | 4 | PostgreSQL AST parser |
| `zip` | 6 | DBeaver import |
| `reqwest` | 5 | Driver download |
| `indoc` | 6 | Help text / skill templates |
| `similar` | 6 | Diff output en agent install |

## Sidecar Java dependencies

| Artefacto | Propósito |
|---|---|
| `jackson-databind` | JSON serialization |
| `org.postgresql:postgresql` | Solo para compilar (no distribuir) |
| `picocli` (opcional) | CLI arg parsing |

Sin Spring Boot, sin frameworks pesados. Main loop cíclico con `ObjectMapper`.
