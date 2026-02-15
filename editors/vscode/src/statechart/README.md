# StateChart Editor - Guía de Uso

**Editor visual de diagramas UML StateChart con ejecución en simulación y hardware real.**

## 📖 Índice

- [Características Implementadas](#-características-implementadas)
- [Cómo Usar](#-cómo-usar) - Guía rápida para usuarios
- [Modos de Ejecución](#-modos-de-ejecución) - Simulación vs Hardware
- [💡 Ejemplo: Traffic Light](#-ejemplo-traffic-light)
- [🔧 Development with Hardware](#-development-with-hardware-desarrollo-con-hardware-real) - **Guía completa para desarrolladores**
- [Próximos Pasos](#-próximos-pasos-mejoras-futuras)
- [Archivos del Proyecto](#-archivos-del-proyecto)
- [Debugging](#-debugging)
- [Action Mappings para Hardware](#-action-mappings-para-hardware)
- [Referencias](#-referencias)

---

## 🎉 Características Implementadas

### 1. **Editor Visual Completo**
- ✅ Crear/editar estados (Normal, Initial, Final, Compound)
- ✅ Agregar transiciones con eventos
- ✅ Editar entry/exit actions
- ✅ Panel de propiedades completo
- ✅ **Panel de Action Mappings** (nueva funcionalidad - configuración visual de hardware)
- ✅ Validación y advertencias para acciones sin mapear
- ✅ Auto-layout y controles de zoom

### 2. **Sistema de Ejecución**
- ✅ Ejecución en **modo Simulación** (sin hardware)
- ✅ Ejecución en **modo Hardware** (EtherCAT, GPIO, etc.)
- ✅ Botón **Run** para iniciar la máquina de estados
- ✅ Botón **Stop** para detener y liberar I/O
- ✅ Visualización del **estado actual** con indicador animado
- ✅ Lista de **eventos disponibles** desde el estado actual
- ✅ Envío de eventos con botones
- ✅ Campo para **eventos personalizados**
- ✅ **Highlight visual** del estado activo en el diagrama (verde con animación)
- ✅ **Transiciones automáticas** con timers (`after` field)

## 🚀 Cómo Usar

### Paso 1: Iniciar el Editor
1. Abre VS Code en: `/home/runtimevic/Descargas/trust-platform/editors/vscode`
2. Presiona **F5** para iniciar Extension Development Host
3. En la ventana de desarrollo, abre un ejemplo:
   ```
   /home/runtimevic/Descargas/trust-platform/examples/statecharts/traffic-light.statechart.json
   ```

### Paso 2: Editar el StateChart
- **Agregar estados**: Usa los botones `➕ State`, `🟢 Initial`, `🔴 Final`
- **Conectar transiciones**: Arrastra desde un estado a otro
- **Editar propiedades**: Selecciona un nodo y edita en el panel derecho inferior
- **Agregar actions**: En el panel de propiedades, usa `➕` para agregar entry/exit actions

### Paso 3: Ejecutar
1. **Presiona ▶️ Run** en el panel superior derecho
2. El estado inicial se resaltará en verde con animación pulsante
3. Los **eventos disponibles** aparecerán como botones
4. **Click en un evento** para transicionar
5. El diagrama se actualizará mostrando el nuevo estado activo

### Paso 4: Simulación
- Usa los botones de eventos para simular transiciones
- O escribe un evento personalizado en el campo "Send Custom Event"
- Observa cómo el estado cambia en tiempo real

## 📊 Layout del Editor

```
┌────────────────────────────────────────┬──────────────────┐
│                                        │  Execution Panel │
│         Diagrama Visual                │  • Run/Stop      │
│         (ReactFlow)                    │  • Estado Actual │
│         • Estados                      │  • Eventos       │
│         • Transiciones                 │  • Custom Event  │
│         • Toolbar                      │                  │
│                                        ├──────────────────┤
│                                        │ Properties Panel │
│                                        │  • Label         │
│                                        │  • Type          │
│                                        │  • Entry Actions │
│                                        │  • Exit Actions  │
└────────────────────────────────────────┴──────────────────┘
```

## 🎯 Modos de Ejecución

### 🖥️ Modo Simulación
- ✅ Ejecución completa del statechart en memoria (TypeScript)
- ✅ Transiciones entre estados con eventos
- ✅ Logs de entry/exit actions en la consola
- ✅ **Timers automáticos** configurables en transiciones (campo `after` en ms)
- ✅ Evaluación de guardas (siempre true en simulación)
- ✅ Detección de estados finales
- ✅ **No requiere hardware** - perfecto para testing y desarrollo

### 🔌 Modo Hardware
- ✅ Conexión con **trust-runtime** vía control endpoint
- ✅ Soporte para **Unix socket** (`/tmp/trust-debug.sock`) y **TCP**
- ✅ **Forzado de I/O** (`io.force`) para control directo de outputs
- ✅ **Lectura de I/O** (`io.read`) para evaluación de guardas
- ✅ **Evaluación de guardas con I/O real**: `%IX0.0 == TRUE`, `%IW0 > 100`, etc.
- ✅ **Action mappings** para mapear actions a direcciones físicas (%QX, %IW, etc.)
- ✅ **Limpieza automática** de I/O al detener (unforce)
- ✅ **Boolean → String conversion** ("TRUE"/"FALSE" para trust-runtime)
- ✅ Soporta **EtherCAT**, **GPIO**, y otros drivers de trust-runtime

## 🔧 Próximos Pasos: Mejoras Futuras

### Context y Variables de Estado
- [ ] Soporte para variables de contexto persistente
- [ ] Scripting en actions (ej: `motorSpeed += 10`)
- [ ] Guardar/restaurar estado del statechart

### Visualización Mejorada
- [ ] Histórico de transiciones
- [ ] Timeline de eventos
- [ ] Gráficos de valores en tiempo real

### Estados Compuestos (Hierarchical States)
- [ ] Implementar nested states
- [ ] History states (shallow/deep)
- [ ] Parallel states (regiones ortogonales)

### Testing y Validación
- [ ] Test runner para statecharts
- [ ] Validación de cobertura de estados
- [ ] Replay de secuencias de eventos

## 📁 Archivos del Proyecto

```
editors/vscode/src/statechart/
├── stateChartEditor.ts          # Provider principal (backend)
├── stateMachineEngine.ts        # Motor de ejecución (sim + hardware)
├── runtimeClient.ts             # Cliente para trust-runtime control endpoint
├── importStatechart.ts          # Comando de importación
├── newStatechart.ts             # Comando crear nuevo
├── README.md                    # Esta documentación
└── webview/
    ├── StateChartEditor.tsx     # Componente principal
    ├── StateNode.tsx            # Nodo visual con animación
    ├── PropertiesPanel.tsx      # Panel de edición
    ├── ExecutionPanel.tsx       # Panel de ejecución
    ├── types.ts                 # Tipos TypeScript
    ├── index.html               # Template HTML
    ├── main.tsx                 # Entry point
    └── hooks/
        └── useStateChart.ts     # Hook para manejo de estado
```

## 🐛 Debugging

Para ver los logs de ejecución:
1. En VS Code (ventana de desarrollo), abre la consola: **Help > Toggle Developer Tools**
2. Tab **Console**
3. Verás logs como:
   ```
   Transitioned from Red to Green via TIMER
   Executing action: turnOnGreenLight
   ```

## 💡 Ejemplo: Traffic Light

El ejemplo del semáforo demuestra:
- **3 estados**: Red, Green, Yellow
- **1 evento**: TIMER (disponible en todos los estados)
- **Entry actions**: Enciende la luz correspondiente
- **Exit actions**: Apaga la luz
- **Ciclo completo**: Red → Green → Yellow → Red

### Prueba rápida:
1. Abre `traffic-light.statechart.json`
2. Presiona **Run**
3. Click en **TIMER** repetidamente
4. Observa el estado cambiar en el diagrama

---

## 🔧 Development with Hardware (Desarrollo con Hardware Real)

Esta sección documenta el flujo completo para **desarrollar y probar** StateCharts con hardware real (EtherCAT, GPIO, etc.).

### Arquitectura

```
┌─────────────────────┐         ┌──────────────────────┐
│  VS Code Extension  │         │   trust-runtime      │
│  (Development Host) │◄───────►│   + Hardware Driver  │
│                     │  Socket │   (EtherCAT/GPIO)    │
│  • StateChart Editor│         │                      │
│  • RuntimeClient    │         │  • Control Endpoint  │
│  • Hardware Mode    │         │  • I/O Forcing       │
└─────────────────────┘         └──────────────────────┘
                                          │
                                          ▼
                                   ┌──────────────┐
                                   │   Hardware   │
                                   │  EK1100 +    │
                                   │  EL2008      │
                                   └──────────────┘
```

### Requisitos Previos

1. **Hardware configurado** (EtherCAT EK1100 + EL2008 o similar)
2. **trust-runtime compilado** (preferiblemente desde source para última versión)
3. **Proyecto backend** en `examples/statechart_backend/`
4. **Permisos** para acceder a hardware (sudo o permisos de red)

### Flujo Completo de Desarrollo

#### 1️⃣ Iniciar el Backend Runtime

El backend proporciona:
- Minimal ST program (define variables I/O)
- Driver configuration (EtherCAT/GPIO)
- Control endpoint para comunicación con VS Code

```bash
# Terminal 1: Start backend
cd examples/statechart_backend
sudo ./start.sh
```

**Salida esperada:**
```
🔨 Compilando proyecto...
✅ Build complete
🚀 Starting runtime...
   Control endpoint: /tmp/trust-debug.sock
⏳ Waiting for socket...
✅ Control endpoint ready: /tmp/trust-debug.sock (rw-rw-rw-)
✅ Backend is running! (PID: 89978)

Press Ctrl+C to stop
```

**Verificar que el socket está listo:**
```bash
ls -l /tmp/trust-debug.sock
# Output: srw-rw-rw- 1 root root 0 feb 15 10:30 /tmp/trust-debug.sock
```

#### 2️⃣ Abrir el Proyecto en VS Code

```bash
# Terminal 2: Open VS Code
cd editors/vscode
code .
```

#### 3️⃣ Iniciar Extension Development Host

En VS Code:
1. Presiona **F5** (o **Run > Start Debugging**)
2. Espera a que se abra la ventana **[Extension Development Host]**
3. En esta nueva ventana, trabajarás con la extensión en desarrollo

**Tip:** La primera vez puede tardar unos segundos en compilar TypeScript.

#### 4️⃣ Abrir un Ejemplo de StateChart

En la ventana **Extension Development Host**:

**Opción A: Navegación Manual**
```
File > Open File... (Ctrl+O)
→ Navega a: trust-platform/examples/statecharts/
→ Selecciona: ethercat-snake.statechart.json
```

**Opción B: Comando Rápido**
```
Ctrl+P → escribe: ethercat-snake.statechart.json
```

**Opción C: Workspace**
```
File > Open Folder...
→ Selecciona: trust-platform/examples/statecharts/
→ Luego abre el archivo .statechart.json
```

#### 5️⃣ Configurar Modo Hardware

En el panel **Execution** (esquina superior derecha):

1. **Selecciona el modo:**
   - 🖥️ **Simulation** (sin hardware) ← por defecto
   - 🔌 **Hardware** (hardware real) ← selecciona este

2. **Verifica la conexión:**
   - El editor intentará conectar a `/tmp/trust-debug.sock`
   - Busca en la consola: `✅ Connected to trust-runtime: unix:///tmp/trust-debug.sock`

#### 6️⃣ Ejecutar y Probar

1. **Click en ▶️ Start Hardware**
2. Si la conexión es exitosa, verás el estado inicial resaltado en **verde**
3. **Eventos automáticos:**
   - Si el StateChart tiene `"after": 200` en las transiciones, avanzará automáticamente
   - Para `ethercat-snake.statechart.json`: avanza cada 200ms
4. **Eventos manuales:**
   - Click en botones de eventos (ej: `START`, `TIMER`)
   - O escribe evento personalizado y click **Send**

#### 7️⃣ Ver Logs de Hardware

Abre **Developer Tools Console** en la ventana Extension Development Host:

```
Help > Toggle Developer Tools > Tab: Console
```

**Logs típicos en modo Hardware:**
```javascript
✅ Connected to trust-runtime via Unix socket: /tmp/trust-debug.sock
🎯 StateMachine initialized in hardware mode
⏰ Auto-firing TIMER after 200ms
🔌 [HW] turnOn_DO0 → FORCE true to %QX0.0
✅ Forced true to %QX0.0
🔌 [HW] turnOff_DO0 → FORCE false to %QX0.0
Transitioned from S1_LED0_On to S2_LED0_1_On via TIMER
```

**Compara con modo Simulation:**
```javascript
🖥️  [SIM] Executing action: turnOn_DO0
🖥️  [SIM] Executing action: turnOff_DO0
Transitioned from S1_LED0_On to S2_LED0_1_On via TIMER
```

#### 8️⃣ Detener Ejecución

En el Execution Panel:
- Click **⏹️ Stop**
- Esto libera los I/O forzados (unforce)
- El control vuelve al programa ST

**Log esperado:**
```javascript
🧹 Releasing 8 forced addresses...
✅ Unforced %QX0.0
✅ Unforced %QX0.1
...
```

#### 9️⃣ Desarrollo Iterativo

**Para hacer cambios:**

1. **Editar StateChart**: Modifica el JSON o usa el editor visual
2. **Guardar** (Ctrl+S)
3. **Recargar Webview**:
   - En paleta de comandos (Ctrl+Shift+P)
   - Busca: `Developer: Reload Webviews`
   - O cierra y reabre el archivo

4. **Re-ejecutar** con ▶️ Start Hardware

**No necesitas recompilar** a menos que cambies código TypeScript de la extensión.

### Troubleshooting Común

#### ❌ "Cannot connect to /tmp/trust-debug.sock"

**Causa:** Backend no está corriendo o socket no existe.

**Solución:**
```bash
# Verifica proceso
ps aux | grep trust-runtime

# Verifica socket
ls -l /tmp/trust-debug.sock

# Reinicia backend
sudo pkill -9 trust-runtime
cd examples/statechart_backend
sudo ./start.sh
```

#### ❌ "EACCES: Permission denied /tmp/trust-debug.sock"

**Causa:** Socket creado por root sin permisos.

**Solución:**
```bash
# Opción 1: Fix permissions
sudo chmod 666 /tmp/trust-debug.sock

# Opción 2: start.sh debería hacerlo automáticamente
# Si no lo hace, verifica que tenga este código:
# chmod 666 /tmp/trust-debug.sock
```

#### ❌ Los LEDs no se encienden

**Diagnóstico:**

1. **Verifica logs de hardware:**
   ```
   🔌 [HW] turnOn_DO0 → FORCE true to %QX0.0
   ✅ Forced true to %QX0.0
   ```

2. **Verifica que trust-runtime tiene acceso a hardware:**
   ```bash
   # En otra terminal
   cd examples/statechart_backend
   sudo /path/to/trust-runtime run --project . --verbose
   ```

3. **Verifica action mappings** en el .statechart.json:
   ```json
   "actionMappings": {
     "turnOn_DO0": {
       "action": "WRITE_OUTPUT",  // ✅ Correcto
       "address": "%QX0.0",
       "value": true
     }
   }
   ```

#### ❌ La extensión no se actualiza después de cambios

**Causa:** Necesitas recompilar TypeScript si cambiaste código de la extensión.

**Solución:**
```bash
cd editors/vscode
npm run compile
# Luego en VS Code: Ctrl+Shift+F5 (Restart Debugging)
```

### Estructura de Archivos para Hardware

```
examples/
├── statechart_backend/           # Backend runtime (REQUERIDO)
│   ├── src/
│   │   ├── Main.st              # Programa ST mínimo
│   │   └── config.st            # Configuración VAR_CONFIG
│   ├── io.toml                  # Driver EtherCAT/GPIO
│   ├── runtime.toml             # Control endpoint config
│   ├── start.sh                 # Script de inicio
│   └── README.md
│
└── statecharts/                  # Ejemplos StateChart
    ├── ethercat-snake.statechart.json        # 16 estados
    ├── ethercat-snake-simple.statechart.json # 5 estados
    └── ethercat-snake-bidirectional.statechart.json # 15 estados
```

### Tips de Desarrollo

**🎨 Visualización:**
- El estado activo se resalta en **verde** con **animación pulsante**
- Usa zoom (rueda del ratón) para mejor vista
- Auto-layout: Click en botón de organización

**⚡ Transiciones Automáticas:**
- Agrega `"after": 200` a transiciones para auto-avance
- Útil para animaciones tipo snake
- Milisegundos: 200 = avanza cada 0.2 segundos

**🔍 Debug:**
- Console logs muestran cada transición
- En modo Hardware: mensajes `🔌 [HW]` confirman escritura I/O
- En modo Simulation: mensajes `🖥️ [SIM]` son solo logs

**📦 Action Mappings:**
- `WRITE_OUTPUT`: Escribe output digital (%QX)
- `SET_MULTIPLE`: Escribe múltiples outputs atomically
- `LOG`: Solo mensaje de consola
- Valores: strings `"TRUE"` o `"FALSE"` (no booleanos)

### Documentación Relacionada

- **[examples/statecharts/README.md](../../../examples/statecharts/README.md)**: Guía de ejemplos
- **[examples/statecharts/HARDWARE_EXECUTION.md](../../../examples/statecharts/HARDWARE_EXECUTION.md)**: Setup hardware para usuarios finales
- **[examples/statechart_backend/README.md](../../../examples/statechart_backend/README.md)**: Configuración backend

---

## 🔗 Referencias

- **XState JSON Format**: Compatible con [XState](https://xstate.js.org/)
- **ReactFlow**: [Documentación](https://reactflow.dev/)
- **trust-platform Runtime**: Ver `crates/trust-runtime/`
- **Proyecto control** (referencia): `/home/runtimevic/Descargas/control`

## 📝 Action Mappings para Hardware

Los **action mappings** conectan las actions de tu StateChart con direcciones I/O reales en trust-runtime.

### 🎨 Editor Visual de Action Mappings (NUEVO)

Ahora puedes **editar los action mappings visualmente** desde el panel integrado en el editor:

1. **Ubicación**: Panel colapsable en la parte inferior del sidebar derecho
2. **Características**:
   - ⚠️ **Advertencias automáticas** para acciones sin mapear (badge naranja)
   - ✏️ **Editar mappings existentes**: Haz clic en cualquier mapping para editarlo
   - ➕ **Agregar nuevos mappings**: Botón "+ Add" en el header
   - 🗑️ **Eliminar mappings**: Botón "Delete" en el editor de mapping
   - 📋 **Desplegable de direcciones**: Selecciona %QX0.0 a %QX0.7 (EL2008)
   - 🔘 **Toggle ON/OFF**: Para valores booleanos de WRITE_OUTPUT
   - 🔍 **Detección de acciones no usadas**: Marca mappings que no están referenciados

3. **Flujo de trabajo recomendado**:
   - Diseña tu StateChart y agrega actions a los estados (entry/exit)
   - Abre el panel "Action Mappings" (expandir si está colapsado)
   - El panel mostrará advertencias para acciones sin mapear
   - Haz clic en "+ Add" o selecciona un mapping existente para editarlo
   - Configura: tipo de acción, dirección hardware, valor
   - Guarda (el mapping se actualiza automáticamente en el JSON)

**Nota**: El editor visual es ideal para WRITE_OUTPUT, LOG y WRITE_VARIABLE. Para SET_MULTIPLE con muchos targets, edita el JSON directamente.

### Formato JSON (alternativa manual)

También puedes editar los action mappings directamente en el archivo `.statechart.json`:

```json
{
  "id": "my-statechart",
  "states": {
    "LED_On": {
      "entry": ["turnOn_LED"],
      "exit": ["turnOff_LED"]
    }
  },
  "actionMappings": {
    "turnOn_LED": {
      "action": "WRITE_OUTPUT",
      "address": "%QX0.0",
      "value": true
    },
    "turnOff_LED": {
      "action": "WRITE_OUTPUT",
      "address": "%QX0.0",
      "value": false
    }
  }
}
```

### Tipos de Actions Soportadas

#### WRITE_OUTPUT - Output Digital
```json
"activateValve": {
  "action": "WRITE_OUTPUT",
  "address": "%QX0.5",
  "value": true
}
```
- Escribe a un output digital
- Valores: `true` o `false` (se convierten a "TRUE"/"FALSE" internamente)

#### SET_MULTIPLE - Múltiples Outputs
```json
"resetAll": {
  "action": "SET_MULTIPLE",
  "targets": [
    { "address": "%QX0.0", "value": false },
    { "address": "%QX0.1", "value": false },
    { "address": "%QX0.2", "value": false }
  ]
}
```
- Escribe múltiples outputs atomically
- Útil para inicialización o apagado de grupos

#### LOG - Mensaje de Consola
```json
"logStatus": {
  "action": "LOG",
  "message": "🚦 Entering Safe State"
}
```
- Solo imprime en consola
- Útil para debugging

### Direcciones IEC 61131-3

- **Digital Outputs:** `%QX0.0` a `%QX0.7` (EL2008 tiene 8 outputs)
- **Digital Inputs:** `%IX0.0` a `%IX0.7`
- **Analog Outputs:** `%QW0`, `%QW1`, etc.
- **Analog Inputs:** `%IW0`, `%IW1`, etc.

### Guardas con Inputs ✅ IMPLEMENTADO

 Las guardas ahora soportan lecturas de I/O reales en modo hardware:

**Ejemplos de Guardas Soportadas:**
```json
{
  "on": {
    "START": {
      "target": "Running",
      "guard": "%IX0.0 == TRUE"
    },
    "STOP": {
      "target": "Idle",
      "guard": "%IX0.1"
    },
    "OVERHEAT": {
      "target": "Emergency",
      "guard": "%IW0 > 100"
    }
  }
}
```

**Operadores Soportados:**
- `==` - Igual a
- `!=` - Diferente de
- `>` - Mayor que
- `>=` - Mayor o igual
- `<` - Menor que
- `<=` - Menor o igual

**Valores Soportados:**
- Booleanos: `TRUE`, `FALSE`
- Números: `100`, `-5`, `3.14`
- Lecturas directas: `%IX0.0` (se evalúa como booleano)

**Comportamiento:**
- En **modo simulación**: Las guardas siempre retornan `true`
- En **modo hardware**: Se leen los valores reales de I/O desde trust-runtime y se evalúa la expresión
- Si una guarda bloquea la transición, aparece en los logs: `Guard %IX0.0 == TRUE blocked transition`

### Timers Automáticos ✅ IMPLEMENTADO

Ahora puedes configurar **auto-transiciones** con delays:

```json
{
  "Red": {
    "on": {
      "TIMER": {
        "target": "Green",
        "after": 3000
      }
    }
  }
}
```

- El campo `after` se configura en **milisegundos**
- Se puede editar visualmente en el **Properties Panel** al seleccionar una transición
- El timer se activa automáticamente al entrar al estado
- En el editor, aparece un campo "Auto-Transition Timer (ms)" al seleccionar una arista

---

**¿Preguntas?** El código está comentado y listo para extender. La arquitectura está diseñada para facilitar la integración con trust-runtime.
