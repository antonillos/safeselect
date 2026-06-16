# SafeSelect: MCP SQL Fail-Closed para Agentes de IA

## Resumen

SafeSelect es un CLI/MCP en Rust que actúa como proxy SQL seguro para agentes de IA. No incluye drivers JDBC. Usa un sidecar Java provisto por SafeSelect para conectividad JDBC, y drivers aportados por el usuario o la empresa. Fail-closed: ante cualquier duda o incidente de seguridad, el proceso se termina por completo.

---

## Arquitectura

### Stack tecnológico
- **Lenguaje principal**: Rust (binary CLI + MCP server)
- **Sidecar**: Java (JDBC proxy, distribuido con SafeSelect)
- **Formato de config**: TOML jerárquico
- **Comunicación Rust ↔ Java**: stdin/stdout con JSON-lines

### Comunicación Rust ↔ Java
- Sin red, sin sockets, sin puertos
- Protocolo: mensajes JSON delimitados por nueva línea
- Lifecycle: Rust lanza el sidecar como subproceso, stdin/stdout pipeado
- Ejemplo:
  ```
  → {"id":1,"method":"execute","params":{"sql":"SELECT * FROM users LIMIT 10"}}
  ← {"id":1,"result":{"columns":["id","name"],"rows":[[1,"Alice"]]}}
  → {"id":2,"method":"ping"}
  ← {"id":2,"result":"pong"}
  ```

### Sidecar Java
- Se compila aparte y se embeble en el Rust binary con `include_bytes!`
- Al primer `serve` se extrae a `~/.local/share/safeselect/sidecar/safeselect-sidecar.jar`
- No contiene ningún driver JDBC
- Dependencias mínimas: solo JDK estándar

### Módulos del crate Rust

```
safeselect/
├── Cargo.toml
├── src/
│   ├── main.rs              # clap entrypoint, dispatch
│   ├── cli.rs               # CLI tree completo
│   ├── config/
│   │   ├── mod.rs           # loader + validator
│   │   ├── project.rs       # project.toml parser
│   │   ├── environment.rs   # environments/<env>.toml parser
│   │   └── driver.rs        # drivers/<vendor>.toml parser
│   ├── mcp.rs               # MCP server JSON-RPC sobre stdio
│   ├── sidecar/
│   │   ├── mod.rs           # lifecycle: start, stop, health, execute
│   │   └── protocol.rs      # tipos para JSON-lines
│   ├── security/
│   │   ├── mod.rs           # orchestrator
│   │   ├── parser.rs        # AST validation via pg_query-rs
│   │   └── policy.rs        # allowlists, denied relations, limits
│   ├── audit.rs             # JSON audit log writer
│   ├── agents.rs            # detect/install/uninstall/status para cada cliente MCP
│   ├── dbeaver.rs           # import DBeaver ZIP (sin credentials ni drivers)
│   └── error.rs             # unified error type (thiserror)
├── sidecar/                 # código fuente Java del sidecar
│   └── src/main/java/.../
├── skills/
│   └── safeselect.md        # skill manifest para OpenCode
└── tests/
    └── integration/
```

---

## CLI

```text
safeselect serve --project <name> --environment <env>
    Inicia el MCP server. Entrypoint principal.

safeselect config validate [--project <name>]
    Valida toda la configuración sin iniciar el servidor.

safeselect config show [--project <name>]
    Muestra la configuración resuelta (sin secretos).

safeselect driver add --vendor <name> --path <jar> --class <driver-class> [--sha256 <hash>]
    Registra un driver JDBC. Sin --sha256 lo calcula automáticamente y lo guarda.

safeselect driver list
    Lista drivers registrados globalmente.

safeselect driver download --vendor postgresql
    Descarga el driver oficial de PostgreSQL (curl desde url conocida).

safeselect agent detect
    Detecta clientes MCP instalados (opencode, copilot, cursor, etc.).

safeselect agent install <cliente> --project <proyecto> --environment <entorno> --name <nombre>
    Instala la entrada MCP en el cliente detectado (con backup atómico).

safeselect agent uninstall <cliente> --name <nombre>
    Elimina exclusivamente la entrada gestionada por SafeSelect.

safeselect agent status
    Muestra estado de las integraciones instaladas.

safeselect import dbeaver <path-to-zip>
    Importa configuración desde ZIP de DBeaver (solo conexiones, sin credenciales).

safeselect check [--project <name> --environment <env>]
    Test de conexión: valida config, secreto, sidecar, driver y reachabilidad.
```

---

## Configuración

### Layout

```
~/.config/safeselect/
├── drivers/
│   └── postgresql.toml          # registro de driver JDBC global
├── projects/
│   └── ecommerce/               # nombre de carpeta = nombre de proyecto
│       ├── project.toml          # política máxima (seguridad + límites)
│       └── environments/
│           ├── testing.toml
│           ├── staging.toml
│           └── production.toml
```

### project.toml — Política máxima (inmodificable por entornos)

```toml
version = 1
display_name = "Ecommerce"

[security]
read_only = true
allowed_schemas = ["reporting", "public"]
denied_relations = ["public.users_credentials"]
allow_volatile_functions = false
require_single_statement = true

[limits]
statement_timeout_ms = 5000
connection_timeout_ms = 5000
max_rows = 500
max_result_bytes = 2_000_000

[audit]
enabled = true
directory = "~/.local/state/safeselect/audit/ecommerce"
max_file_bytes = 10_000_000
retain_files = 10
```

### environments/<env>.toml — Conexión + endurecimiento

```toml
version = 1

[database]
driver = "postgresql"
url = "jdbc:postgresql://db.testing.internal:5432/ecommerce"
username = "reporting_reader"

[database.secret]
source = "macos-keychain"
service = "safeselect"
account = "ecommerce/testing"

[tls]
mode = "verify-full"
ca_file = "/opt/company/certs/internal-ca.pem"

[ssh]
enabled = true
host = "bastion.testing.example.com"
port = 22
username = "deploy"
identity_file = "~/.ssh/id_ed25519"
known_hosts = "~/.ssh/known_hosts"

[limits]
statement_timeout_ms = 3000
max_rows = 200
```

### drivers/<vendor>.toml — Registro global

```toml
version = 1
vendor = "postgresql"
path = "/opt/company/jdbc/postgresql.jar"
class = "org.postgresql.Driver"
sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
```

### Reglas de configuración

- El nombre de la carpeta es el identificador del proyecto
- No se permiten contraseñas en TOML
- Las URLs JDBC no pueden contener credenciales
- `production` no tiene tratamiento implícito especial
- Ningún entorno puede ampliar schemas, rows, timeout o capacidades definidas en `project.toml`
- Configuración inválida, permisos inseguros o campos desconocidos impiden iniciar SafeSelect

---

## MCP Tools

### `select`

```json
{
  "name": "select",
  "arguments": {
    "sql": { "type": "string", "description": "SQL SELECT query" }
  }
}
```

Respuesta: columnas + filas, truncado según `max_rows` y `max_result_bytes`.

### `list_tables`

```json
{
  "name": "list_tables",
  "arguments": {
    "schema": { "type": "string", "optional": true, "description": "Schema filter" }
  }
}
```

### `explain`

```json
{
  "name": "explain",
  "arguments": {
    "sql": { "type": "string", "description": "SQL to EXPLAIN" }
  }
}
```

Retorna plan de ejecución. No ejecuta la query real.

---

## Seguridad

### Pipeline de validación (orden estricto)

1. **Parseo AST** con `pg_query-rs` (libpg_query de PostgreSQL)
2. **Validaciones**:
   - Sentencia única (si `require_single_statement = true`)
   - Solo `SELECT` / `EXPLAIN` (si `read_only = true`)
   - Esquemas permitidos
   - Relaciones denegadas
   - Funciones volátiles prohibidas
   - Sin `COPY`, `SET`, `PREPARE`, etc.
3. **Ejecución**: transacción JDBC `READ ONLY` + `statement_timeout`
4. **Post-ejecución**: verificación de límites (rows, bytes)

### Fail-closed

Ante cualquier violación o error de integridad:
1. Cancelación de la consulta
2. Cierre de conexión JDBC
3. Terminación del sidecar Java
4. Alerta redactada en stderr
5. Terminación del proceso MCP

Sin reintentos automáticos tras incidentes.

### Audit log

- Formato: JSON
- Campos: timestamp, cliente MCP (del init), proyecto, entorno, categoría, decisión, hash SHA-256 de la consulta
- Nunca se registra SQL completo, secretos, DSN ni credenciales
- Si audit no puede inicializarse, no se abre la conexión

---

## Secretos

### Fuente única y explícita

Cada `[database.secret]` declara una única fuente:

```toml
[database.secret]
source = "macos-keychain"
service = "safeselect"
account = "ecommerce/testing"
```

```toml
[database.secret]
source = "env"
variable = "SAFESELECT_ECOMMERCE_TESTING_PASSWORD"
```

- Sin fallback automático. Si la fuente falla → error, no continúa
- No hay contraseñas en TOML ni en URLs JDBC
- `macos-keychain` usa `security find-generic-password` (con soporte futuro via `keychain-rs` crate)

---

## SSH Bastion

SafeSelect **no gestiona túneles SSH**. Solo:

1. Al cargar config con `[ssh] enabled = true`, emite un WARN en stderr
2. Antes de conectar, hace TCP connect al host:puerto del JDBC URL
3. Si falla: error claro con instrucciones de cómo abrir el túnel
4. Si funciona: continúa normalmente

El usuario es responsable de mantener el túnel activo.

---

## Drivers

### Sin drivers embebidos

- SafeSelect incluye **cero drivers JDBC**
- Registro obligatorio: `safeselect driver add --vendor postgresql --path /ruta/al.jar --class org.postgresql.Driver`
- SHA-256 se calcula automáticamente si no se provee, y se valida en cada `serve`
- `safeselect driver download --vendor postgresql` descarga el driver oficial desde URL conocida
- El driver se valida en cada inicio: checksum, permisos, existencia

---

## Agentes Compatibles

### v1

- OpenCode
- OpenAI Codex
- Claude Code
- GitHub Copilot (VS Code)
- Cursor
- Windsurf
- Gemini CLI

### Instalación

1. Detecta versión y ubicación de configuración del cliente
2. Valida formato compatible
3. Muestra diff exacto del cambio
4. Solicita confirmación
5. Crea copia de seguridad privada
6. Escribe atómicamente preservando otras entradas
7. Relee y valida el resultado

Cada entrada MCP fija proyecto y entorno:

```json
{
  "command": "safeselect",
  "args": ["serve", "--project", "ecommerce", "--environment", "testing"]
}
```

### OpenCode skill

`safeselect` distribuye su skill manifest. Los agentes OpenCode descubren SafeSelect escaneando skills instalados. El skill describe las herramientas MCP, comandos de instalación y ejemplos de uso.

---

## Distribución

### Release binary vía cargo-dist

- Binario único para Linux (x86_64, aarch64) y macOS (x86_64, aarch64)
- El sidecar Java va embebido en el binario
- SHA-256 checksums en cada release

### Homebrew

```ruby
class Safeselect < Formula
  desc "MCP SQL Fail-Closed for AI Agents"
  homepage "https://github.com/anomalyco/safeselect"
  url "..."
  sha256 "..."

  depends_on "openjdk@17"

  def install
    bin.install "safeselect"
  end
end
```

### asdf

Plugin en repo separado `https://github.com/anomalyco/asdf-safeselect` con `bin/install`, `bin/list-all`, `bin/download`.

### CI/CD

- GitHub Actions
  - `cargo test` + lint + format
  - `cargo dist` para builds de release
  - Trigger para actualizar Homebrew formula
  - Publicación de release notes
- Maven/Gradle para sidecar Java (separado, integrado en build Rust)

---

## Testing

### Scope v1
- Parseo y validación de config (proyecto, entorno, driver)
- Instalación/actualización/desinstalación para cada cliente MCP
- Preservación de configs existentes, backups, escritura atómica
- Rechazo de configs ambiguas, corruptas, enlazadas o con permisos inseguros
- Cada entrada MCP fija un único proyecto y entorno
- Driver ausente, checksum alterado, JVM incompatible
- Cierre total ante consultas prohibidas y eventos de seguridad
- Audit log: formato, contenido, rotación
- DBeaver import: válido, malicioso, sin credenciales
- Ausencia de drivers en la distribución

### Stack de testing
- Rust: `#[cfg(test)]` unit tests + `tests/` integration tests
- Sidecar: mock del protocolo JSON-lines para tests sin Java
- Fixtures: configs TOML, configs corruptas, snapshots de clientes MCP
- Prop tests: fuzzing de configs

---

## Fases de Implementación

### Fase 1: Esqueleto + CLI (días 1-2)
- `cargo init`, dependencias (clap, serde, toml, thiserror, tracing)
- CLI tree completo con `clap`
- Loader + validador de config TOML
- Tipos de error unificados

### Fase 2: MCP server (días 3-4)
- JSON-RPC server sobre stdio
- Tools: `select`, `list_tables`, `explain`
- Sesión MCP (initialize, list_tools, call_tool)

### Fase 3: Sidecar Java (días 5-7)
- Sidecar Java mínimo: stdin/stdout JSON-lines
- Protocolo: execute, ping, error response
- Lifecycle desde Rust: start, health check, shutdown
- Extracción del JAR embebido

### Fase 4: Seguridad (días 8-10)
- Integración `pg_query-rs` para AST
- Policy enforcement: schemas, denied relations, single statement, read-only
- Pipeline fail-closed completo
- Audit log writer

### Fase 5: Secretos + Drivers (días 11-12)
- macOS Keychain resolver (CLI `security`)
- Env var resolver
- `safeselect driver add` con SHA-256
- `safeselect driver download`

### Fase 6: Agentes (días 13-15)
- `safeselect agent detect/install/uninstall/status`
- Soporte v1 para todos los clientes
- OpenCode skill manifest
- DBeaver import

### Fase 7: Distribución (días 16-17)
- GitHub Actions + cargo-dist
- Homebrew formula
- asdf plugin
- Documentación de setup

---

## Supuestos v1

- Solo clientes MCP locales vía `stdio`
- Sin soporte de clientes MCP remotos (ChatGPT, etc.)
- PostgreSQL como primer dialecto JDBC
- Solo macOS y Linux. Windows fuera de v1
- Toda modificación de configuración de agente requiere confirmación humana
- SSH bastion es responsabilidad del usuario — SafeSelect solo verifica conectividad
