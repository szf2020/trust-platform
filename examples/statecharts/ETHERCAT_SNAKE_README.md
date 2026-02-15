# EtherCAT Snake Pattern - UML StateChart Examples

Este directorio contiene ejemplos de StateCharts para controlar hardware real EtherCAT, específicamente demostrando un patrón visual tipo "serpiente" o "Knight Rider" en salidas digitales.

## 📁 Archivos Disponibles

### 1. `ethercat-snake.statechart.json`
Patrón serpiente completo con 17 estados:
- **Fase 1 (Estados 0-8)**: Enciende LEDs secuencialmente 0→7
- **Fase 2 (Estados 9-16)**: Apaga LEDs secuencialmente 7→0
- **Ciclo continuo**: Vuelve al estado 1
- **Total**: 16 transiciones por ciclo completo

### 2. `ethercat-snake-bidirectional.statechart.json` ⭐ Recomendado
Patrón bidireccional más realista tipo "Knight Rider":
- **Forward (0-7)**: LED se mueve de izquierda a derecha
- **Backward (6-0)**: LED se mueve de derecha a izquierda
- **Entry/Exit actions**: Cada estado enciende su LED en entry y lo apaga en exit
- **Efecto visual**: Solo un LED encendido a la vez, moviéndose

## 🎯 Hardware Requerido

```
[PC NIC] → [EK1100 Coupler] → [EL2008 DO 8ch]
```

- **EK1100**: EtherCAT Bus Coupler
- **EL2008**: 8 salidas digitales (24V DC)
- **LEDs o cargas**: Conectadas a DO0-DO7

## 🔧 Action Mappings Explicados

Los `actionMappings` conectan las acciones del StateChart con variables físicas de I/O:

```json
{
  "turnOn_DO0": {
    "action": "WRITE_OUTPUT",
    "address": "%QX0.0",    // Dirección IEC 61131-3
    "value": true            // Valor a escribir
  }
}
```

### Tipos de Actions Soportadas

| Action Type | Descripción | Ejemplo |
|-------------|-------------|---------|
| `WRITE_OUTPUT` | Escribe a una salida digital | `%QX0.0 := TRUE` |
| `WRITE_VARIABLE` | Escribe a una variable ST | `motorSpeed := 1500` |
| `SET_MULTIPLE` | Escribe múltiples valores | Apagar todos los LEDs |
| `LOG` | Log de depuración | Mensajes de estado |

### Mapeo de Direcciones

Las direcciones siguen el estándar IEC 61131-3:

```
%QX0.0  →  EL2008 Canal 0 (DO0)
%QX0.1  →  EL2008 Canal 1 (DO1)
%QX0.2  →  EL2008 Canal 2 (DO2)
...
%QX0.7  →  EL2008 Canal 7 (DO7)
```

**Formato**: `%QX[byte].[bit]`
- `Q` = Output
- `X` = Boolean/Bit
- `0` = Byte 0 (primer módulo)
- `.0-.7` = Bits 0-7

## 🚀 Cómo Probar en VS Code (Simulación)

### Paso 1: Abrir el Archivo
```bash
cd /home/runtimevic/Descargas/trust-platform/editors/vscode
code .
# Presiona F5 para Extension Development Host
```

En la ventana de desarrollo:
```
Ctrl+O → examples/statecharts/ethercat-snake-bidirectional.statechart.json
```

### Paso 2: Visualizar el Diagrama
El editor mostrará:
- **Estados**: Forward_0 → Forward_7 → Backward_6 → ... → Backward_0
- **Transiciones**: Evento `TICK` entre estados
- **Actions**: Entry/exit para cada estado

### Paso 3: Ejecutar Simulación
1. **Click en ▶️ Run** (panel derecho superior)
2. **Enviar evento START** para iniciar
3. **Click repetido en TICK** para simular el timer
4. **Observar**: El estado activo se mueve visualmente

### Paso 4: Ver Logs
```
Help > Toggle Developer Tools > Console
```

Verás logs como:
```
Executing action: turnOn_DO0
Executing action: turnOff_DO0
Executing action: turnOn_DO1
```

## 🔌 Cómo Ejecutar con Hardware Real

⚠️ **NOTA**: Para ejecutar con hardware real, necesitas la integración con trust-runtime que aún está en desarrollo.

### Arquitectura Propuesta

```
┌─────────────┐   WebSocket    ┌────────────────┐   Control API   ┌─────────────┐
│  VS Code    │ ←────────────→ │  trust-runtime │ ←──────────────→│  EtherCAT   │
│  StateChart │   Events/State │  + StateMachine│    I/O Updates   │  Hardware   │
└─────────────┘                └────────────────┘                   └─────────────┘
```

### Paso 1: Preparar trust-runtime con StateMachine Support

Necesitas agregar a `trust-runtime`:

```rust
// crates/trust-runtime/src/statechart/mod.rs
pub struct StateMachineRunner {
    machine: StateMachine,
    io_context: Arc<IoContext>,
}

impl StateMachineRunner {
    pub fn execute_action(&mut self, action: &str, mapping: &ActionMapping) {
        match mapping.action.as_str() {
            "WRITE_OUTPUT" => {
                let addr = &mapping.address;
                let value = &mapping.value;
                self.io_context.write_output(addr, value);
            }
            "WRITE_VARIABLE" => {
                // Escribir a variable ST
            }
            _ => {}
        }
    }
}
```

### Paso 2: Configurar io.toml

Crea `examples/statecharts/ethercat-snake-project/src/io.toml`:

```toml
[io]
driver = "ethercat"

[io.params]
adapter = "enp111s0"  # Tu interfaz de red
timeout_ms = 250
cycle_warn_ms = 5
on_error = "fault"

[[io.params.modules]]
model = "EK1100"
slot = 0

[[io.params.modules]]
model = "EL2008"
slot = 1
channels = 8

# Safe state: Apagar todos los LEDs al detener
[[io.safe_state]]
address = "%QX0.0"
value = "FALSE"

[[io.safe_state]]
address = "%QX0.1"
value = "FALSE"

# ... (resto de salidas)
```

### Paso 3: Programa ST Mínimo

Crea `src/Main.st`:

```structured-text
PROGRAM Main
VAR
    (* Las variables son controladas por el StateChart *)
    DO0 AT %QX0.0 : BOOL;
    DO1 AT %QX0.1 : BOOL;
    DO2 AT %QX0.2 : BOOL;
    DO3 AT %QX0.3 : BOOL;
    DO4 AT %QX0.4 : BOOL;
    DO5 AT %QX0.5 : BOOL;
    DO6 AT %QX0.6 : BOOL;
    DO7 AT %QX0.7 : BOOL;
    
    (* Timer para generar eventos TICK *)
    tick_timer : TON;
    tick_interval : TIME := T#200MS;  (* Velocidad del snake *)
END_VAR

(* Generar eventos TICK cada 200ms *)
tick_timer(IN := NOT tick_timer.Q, PT := tick_interval);

(* El StateChart responderá a los eventos TICK *)
(* y actualizará las salidas DO0-DO7 automáticamente *)

END_PROGRAM
```

### Paso 4: Crear config.st

```structured-text
CONFIGURATION Main_Config
    RESOURCE Resource1 ON PLC
        TASK MainTask(INTERVAL := T#10ms, PRIORITY := 1);
        PROGRAM MainProgram WITH MainTask : Main;
    END_RESOURCE
END_CONFIGURATION
```

### Paso 5: Ejecutar

```bash
# Permisos EtherCAT
sudo setcap cap_net_raw,cap_net_admin=eip $(readlink -f $(which trust-runtime))

# Configurar interfaz
sudo nmcli dev set enp111s0 managed no
sudo ip link set enp111s0 up

# Ejecutar
trust-runtime run --project examples/statecharts/ethercat-snake-project \
                  --statechart examples/statecharts/ethercat-snake-bidirectional.statechart.json
```

## 📊 Timing del Patrón Snake

Con `TICK` cada 200ms:

| Fase | Estados | Duración Total |
|------|---------|----------------|
| Forward | 8 estados | 1.6 segundos |
| Backward | 7 estados | 1.4 segundos |
| **Ciclo completo** | **15 transiciones** | **3.0 segundos** |

**Personalizar velocidad**: Ajusta `tick_interval` en `Main.st`

## 🎨 Visualización

### Diagrama del StateChart (Bidireccional)

```
Init
  │ START
  ↓
Forward_0 → Forward_1 → Forward_2 → Forward_3 → Forward_4 → Forward_5 → Forward_6 → Forward_7
   ↑                                                                                      │
   │                                                                                      │ TICK
   │                                                                                      ↓
Backward_0 ← Backward_1 ← Backward_2 ← Backward_3 ← Backward_4 ← Backward_5 ← Backward_6
```

### Efecto Visual en LEDs

```
Forward:
LED: ●○○○○○○○  →  ○●○○○○○○  →  ○○●○○○○○  →  ... →  ○○○○○○○●

Backward:
LED: ○○○○○○○●  →  ○○○○○○●○  →  ○○○○○●○○  →  ... →  ●○○○○○○○
```

## 🔍 Debugging

### Ver Estado Actual
```bash
# Si trust-runtime expone control endpoint
echo '{"cmd":"statechart_status"}' | nc localhost 9000
```

### Logs
```bash
trust-runtime run --log-level debug
```

Verás:
```
[StateChart] Transitioned from Forward_3 to Forward_4 via TICK
[StateChart] Executing action: turnOn_DO4
[StateChart] Executing action: turnOff_DO3
[EtherCAT] Write %QX0.4 = true
```

## 🚨 Troubleshooting

### Error: "No modules found"
→ Verifica que EK1100 y EL2008 estén configurados en `io.toml` en el orden físico correcto

### Error: "Permission denied opening raw socket"
→ Ejecuta: `sudo setcap cap_net_raw,cap_net_admin=eip $(which trust-runtime)`

### Los LEDs no se mueven
→ Verifica que estás enviando eventos `TICK` periódicamente

### Solo un LED parpadea
→ Revisa que las exit actions estén ejecutándose (`turnOff_DOx`)

## 📚 Referencias

- **Proyecto base**: `examples/ethercat_ek1100_elx008_v2/`
- **IEC 61131-3 Addressing**: Ver trust-platform docs
- **XState JSON**: https://xstate.js.org/
- **EtherCAT**: https://www.ethercat.org/

## 🎯 Próximos Ejemplos

Ideas para más StateCharts con hardware:

1. **Traffic Light** → Control de semáforo con entradas de sensores
2. **Motor Control** → Arranque/paro con safety checks
3. **Conveyor Belt** → Control de cinta con sensores de posición
4. **Pick & Place** → Robot simple con secuencia de estados

---

**Estado actual**: Simulación funcional en VS Code. Integración con hardware en desarrollo.

Para probar **ahora mismo**: Abre cualquiera de los archivos `.statechart.json` en VS Code con la extensión y usa el panel de ejecución!
