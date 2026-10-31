# Baseline de documento grande — fixture inicial

Archivo generado: `D:\slate-baselines\large-document-5m.md`.
Está fuera del repositorio Git para evitar que el scope del experimento se amplíe
al proyecto Slate. No se abrió automáticamente ni se modificó ningún documento
existente. El JSON al lado del archivo guarda tamaño, huella y posiciones.

## Datos reproducibles

- **5.000.000 caracteres Unicode** (codepoints).
- **5.191.998 bytes UTF-8**, aproximadamente 4,95 MiB.
- **27.343 líneas**; la más larga tiene **8.119 caracteres**.
- Markdown: headings, párrafos, listas sin checkboxes, tablas, bloques de código
  de ejemplo y líneas largas; español con acentos, griego y caracteres CJK.
- Sin tareas Flow ni URLs externas. Los ejemplos de código son sólo texto.
- SHA-256: `8bfa07e72523ae4853339395c309823c65475fab112dd37a0164ede325f9bcd8`.

Marcas únicas para buscar:

| Texto | Línea (1-based) |
| --- | ---: |
| `SLATE_BASELINE_NEEDLE_START` | 7 |
| `SLATE_BASELINE_NEEDLE_MIDDLE` | 13.673 |
| `SLATE_BASELINE_NEEDLE_END` | 27.343 |

## Primera prueba manual

1. Abrir con el release habitual usando
   `:open D:\slate-baselines\large-document-5m.md`.
2. Anotar versión/release, tamaño de ventana, tamaño de fuente y estado de wrapping.
3. Probar apertura, scroll sostenido y búsqueda de las tres marcas.
4. Para edición, usar Save As con un nombre nuevo. Probar escritura, borrado y undo
   en principio/medio/final y en una línea larga, conservando intacta esta fixture.
5. Comparar wrapping activado/desactivado con la misma ventana y fuente.
6. Separar apertura/indexación de interacción: esperar a que termine cualquier
   escaneo Flow antes de medir edición, búsqueda y construcción de frames.

Las mediciones iniciales y la primera corrección se registran abajo.
El generador escribe el archivo, por lo que puede estar caliente en la
caché del sistema operativo: no llamar "cold open" a esa primera lectura. No se
vacían cachés ni se cambian antivirus/PATH. Los tiempos de CPU de construcción de
UI no equivalen a presentación GPU ni latencia completa de teclado a pantalla.

## Primera medición y corrección — 2026-10-11

Prueba GUI del usuario: log `performance-20261011T064523.942Z-27060.log`.
Lectura: 1,333 ms; preparación completa del worker: 6,382 ms. Aplicación del
resultado: 239,905 ms, incluidos 153,990 ms de indexación Flow en UI y 78,528 ms
hasta dispatch. Después de aplicar el resultado, la construcción de UI tardó
29.220,983 ms; el siguiente frame medido tardó 5.791,689 ms de CPU. El inter-frame
incluye otros 9.233,693 ms que esta instrumentación no atribuye a una causa.

Causa reproducida: wrapping calculado para todas las líneas en cada frame,
con layout de cada prefijo creciente de texto. Eso producía trabajo superlineal
y muchos galleys temporales. El contexto de bloques de código también recorría
el documento repetidamente desde los renglones visibles.

Benchmark aislado del editor, release, ventana lógica 900×560, fuente monoespaciada
12 pt, wrapping y Markdown activados, misma fixture sin editar en disco:

| CPU del editor (ms) | Antes | Después |
| --- | ---: | ---: |
| Primer frame | 18.679,251 | 65,770 |
| Frame con scroll | 1.343,885 | 0,087 |
| Ir al final | 1.392,306 | 0,577 |
| Idle al final | 1.370,801 | 0,112 |
| Insertar un carácter + frame | no medido | 58,578 |
| Undo + frame | no medido | 60,023 |

Son ejecuciones aisladas, no distribuciones estadísticas ni mediciones de GPU,
latencia de input completa o frames de toda `SlateApp`. La lectura y las acciones
Flow no están incluidas. Se requiere retest GUI del usuario antes de atribuir
estos números a su interacción real.

Corrección: medidas de avance de glifos en una pasada, caché derivada de filas
claveada por identidad de buffer/revisión/ancho/fuente/escala/wrap y contexto de
fences indexado por identidad/revisión. No se cachea contenido de archivos. Los
cambios de documento, texto, undo, geometría y fuente invalidan los datos derivados.
Las filas son compartidas sin copiarlas en cada frame. Se redujo el heartbeat
Flow de una escritura por frame a un intento cada 3 segundos por ruta, manteniendo
su ventana de frescura de 10 segundos. No se atribuye un tiempo medido a ese último
cambio.

Validación: 193 pruebas pasan; 1 benchmark manual ignorado en el suite normal.
Pruebas de Unicode/tabs/líneas largas, ancho de texto pintado, invalidación de
layout, fences y protección ante documentos distintos con igual revisión.
Logs nuevos: `editor.layout.rebuild` y `editor.render.slow` (CPU ≥16 ms para
buffers de al menos 1 MB), sin contenido del documento.

Reproducir benchmark desde el proyecto:

```text
cargo test --release --locked large_document_tests::benchmark_large_document_editor -- --ignored --nocapture
```

El usuario confirmó la mejora de apertura/scroll en GUI. Búsqueda, preview,
indexación Flow y guardado siguen pendientes de perfilado.

## Segunda corrección: edición incremental — 2026-10-11

El buffer registra bordes de líneas no modificadas por revisión en un journal
acotado a 64 cambios. Para typing, borrado y pegado por rango, actualiza el índice
de líneas a partir de los newlines de la sustitución y ajusta offsets posteriores,
sin volver a recorrer todos los bytes. Undo/redo calculan el rango modificado
entre snapshots y notifican los mismos bordes.

El layout reutiliza las filas del prefijo/sufijo intactos, recompone sólo las líneas
lógicas afectadas y ajusta índices/offsets de las filas posteriores. Se combinan
cambios ocurridos entre frames. Si faltan entradas del journal, se reemplaza el
documento, hay edición externa desconocida o cambia fuente/ancho/escala/wrap,
se realiza un recálculo completo seguro. No se cachea contenido de archivos.

Benchmark aislado con las mismas condiciones:

| CPU, operación + frame (ms) | Antes incremental | Después |
| --- | ---: | ---: |
| Insertar al final | 58,578 | 2,095 |
| Undo al final | 60,023 | 5,234 |
| Insertar en el medio | no medido | 3,038 |
| Undo en el medio | no medido | 5,834 |
| Insertar al principio | no medido | 2,509 |
| Undo al principio | no medido | 5,785 |
| Pegar varias líneas en el medio | no medido | 3,080 |
| Undo del pegado | no medido | 5,952 |

Primer frame en esta ejecución: 59,204 ms; frame con scroll: 0,092 ms.
No son mediciones de GPU ni de toda la aplicación. El retest GUI se registra abajo.
El archivo fuente y su SHA-256 permanecen sin cambios.

197 pruebas pasan y el benchmark manual continúa ignorado en el suite normal.
Las pruebas nuevas comparan byte a byte los rangos de filas con un layout fresco,
verifican el índice de líneas, edición Unicode, splits/joins, vaciado del buffer,
selección, undo/redo, batched edits, journal agotado y edición externa. Incluyen
400 operaciones pseudoaleatorias reproducibles y el caso de una única línea
recalculada entre mil líneas sin cambiar el resto. El log de layout informa
`recalculated_lines` para auditar el trabajo real.

### Retest GUI de edición

El usuario abrió `large-document-5m - Copy.md`, alternó modificaciones y
movimiento/desplazamiento y confirmó que guardó. Log:
`performance-20261011T071931.861Z-24912.log` (release).

- Lectura: 1,805 ms; worker completo: 6,954 ms.
- Primer layout: 50,577 ms; render del editor: 55,387 ms.
- Solicitud de apertura → fin del siguiente frame de CPU: 184,344 ms;
  ese siguiente frame completo de UI tardó 8,388 ms.
- 36 actualizaciones de layout posteriores: **una sola línea recalculada en
  todas**, entre 0,221 y 0,520 ms; mediana 0,243 ms.
- Sólo un evento `editor.render.slow`: el primer render. No aparecieron otros
  renders de editor de al menos 16 ms en esta ejecución.

El guardado fue confirmado por el usuario, no cronometrado por este log. Los
valores de layout no son la latencia total de la edición ni de presentación GPU.
Búsqueda, preview y trabajo Flow aún no tienen retest de rendimiento específico.

Límites: el vector plano aún copia/ajusta metadata de filas posteriores; el índice
Markdown de fences se invalida por revisión; undo mantiene snapshots de texto
completo y hace diff/reindexado lineal al restaurar. Una línea lógica enormemente
larga aún se recalcula entera. Estas mejoras no implican que todo el pipeline de
edición, Flow, búsqueda o recuperación tenga costo constante.

## Scroll sincronizado bilateral editor/preview

El comportamiento deseado fue aclarado por el usuario: mover cualquiera de los
paneles debe mantener ambos en la misma zona del contenido, no aplicar el mismo
delta de píxeles o el mismo número de líneas. La corrección anterior de ownership
de la rueda permanece: sólo el panel bajo el mouse procesa directamente la rueda;
el otro recibe una posición sincronizada.

El editor expone una posición fuente interpolada dentro de las filas envueltas.
El preview registra anclas con la geometría realmente asignada por egui para
párrafos, títulos, código, espacios y filas de tablas. Se convierte entre ambas
representaciones en las dos direcciones. Las filas de tabla usan su altura real;
el separador Markdown oculto tiene altura cero. No se usa un porcentaje global
del documento ni se copian offsets de píxeles entre paneles.

Los extremos se fijan a inicio/final de cada panel, la geometría se vuelve a
registrar al renderizar y se invalida por documento/revisión/ancho/escala. El scroll
de rueda en el preview se anticipa antes de dibujar el editor para mover ambos
en ese frame cuando la geometría anterior es válida. El scrollbar/inercia también
se detectan por el offset final del preview y solicitan repaint; según el orden
de renderizado, el editor puede actualizarse en el siguiente frame de GUI.

202 pruebas pasan y 1 benchmark sigue ignorado normalmente. Incluye conversión
bilateral con distintas alturas y separadores ocultos, scroll por ambas columnas,
extremos, cambio de escala, edición e inexistencia de deriva entre frames. Falta
retest visual del usuario. No se ha optimizado aquí el renderizado integral del
preview ni se afirma mejora de su rendimiento.

## Resize con preview — 2026-10-11

El usuario detectó resize lento después del ajuste del footer. Log revisado:
`performance-20261011T141427.241Z-28780.log`. Cada variación de ancho reconstruía
las 27.344 líneas del editor (~51–52 ms). El preview además volvía a crear/layout
los widgets de todo el Markdown para cada ancho. El footer no tenía un costo
atribuido en ese log y sus conteos estaban cacheados por revisión.

Benchmark aislado release, editor + preview, mismo documento de 5M caracteres,
ventana de 560 pt de altura. CPU de `ctx.run`, sin GPU/input end-to-end ni footer:

| Ancho (pt) | Antes (ms) | Después (ms) |
| --- | ---: | ---: |
| 900, primer frame | 661,107 | 29,899 |
| 900, repetido | 30,640 | 4,812 |
| 880 | 630,837 | 16,144 |
| 860 | 737,996 | 16,844 |
| 840 | 782,435 | 15,914 |
| 820 | 824,372 | 15,949 |
| 800 | 853,596 | 15,940 |
| 800, repetido | 204,862 | 4,720 |

Corrección: el preview materializa widgets de líneas visibles más overscan.
Las líneas fuera de vista reservan alturas estimadas o reutilizan la medición
anterior; las tablas reservan su altura exacta sin pintar celdas invisibles.
Metadatos de altura se invalidan por identidad/revisión/escala; al cambiar ancho
se ajusta la estimación y se mide de nuevo al entrar en vista. Se mantienen las
anclas bilaterales. En documentos grandes, la extensión global y el thumb pueden
recalibrarse mientras se miden zonas nuevas: **no se promete altura exacta de
párrafos aún no materializados**. El contenido visible usa geometría medida real.
No se conserva texto viejo ni se retrasa el input para esperar el resize.

El wrapping del editor sigue recalculándose al cambiar ancho, pero los avances
ASCII usan una tabla directa en vez de una consulta hash por carácter; Unicode
mantiene las medidas de glifos. Las pruebas existentes comparan el layout
incremental/completo y el ancho pintado para evitar alterar posiciones del texto.

210 pruebas pasan; 2 benchmarks manuales ignorados normalmente. Se agregaron
pruebas de metadatos de altura, materialización de una fracción pequeña de 3.000
párrafos y conservación de anclas al desplazar/cambiar escala. Se corrigió la
fixture de footer para no heredar las preferencias reales del usuario. Nuevo
log `preview.render.slow` informa costo y trabajo realizado, no texto.
Retest GUI de resize pendiente.

```text
cargo test --release --locked resize_tests::benchmark_large_preview_resize -- --ignored --nocapture
```

## Regeneración

Desde `D:\slate`:

```text
python tools/generate_large_document_baseline.py D:/slate-baselines/another-5m.md
```

El contenido es determinista; el generador rechaza destinos o metadata existentes.
El original y su metadata se conservan fuera de Git; el generador queda en el
proyecto para reproducirlo en otra máquina.
