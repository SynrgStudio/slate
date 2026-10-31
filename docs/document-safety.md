# Seguridad y recuperación del texto

## Alcance

El editor principal y el preview guardan con un temporal en el mismo directorio:
escribir todo → sincronizar → volver a verificar el destino → reemplazar
atómicamente. Windows usa `ReplaceFileW` para archivos existentes y `MoveFileExW`
sin reemplazo para nuevos archivos. Nunca se trunca primero el documento.

La huella SHA-256 corresponde a los bytes efectivamente abiertos, no a una lectura
posterior que pudiera pasar por alto un cambio concurrente. El guardado compara
el contenido antes y después de preparar el temporal. También rechaza archivos
readonly, especiales, symlinks/reparse points y hard links: usar Save As con un
nombre nuevo. Guardar como sobre un destino existente se bloquea; no hay
sobrescritura silenciosa ni botón de forzar.

Un monitor independiente comprueba el archivo principal y el preview aproximadamente
cada dos segundos. No recarga automáticamente ni descarta texto. Ante cambios:

- **C / Save copy:** elegir un nombre nuevo; conservar el original externo.
- **R / Reload:** en el principal, confirmar primero el descarte si está modificado.
  Un preview modificado requiere guardar una copia antes de recargarlo.
- **Esc / Keep text:** continuar editando; la protección al guardar sigue vigente.

Un aviso del monitor se muestra una vez por archivo/huella base. Un nuevo intento
de guardado incompatible vuelve a avisar. El monitor no interrumpe Save As.

## Checkpoints

Los buffers modificados principal y preview, además del Scratch independiente,
se copian aproximadamente una vez por segundo cuando cambia el texto. No se
reescriben continuamente si permanecen iguales. El worker conserva como máximo
un checkpoint pendiente por buffer, además del lote en curso; serialización,
escritura y sincronización ocurren en segundo plano. Los monitores y el descubrimiento
de borradores usan workers separados para no retrasar checkpoints por lecturas lentas.

Ubicación Windows: `%LOCALAPPDATA%\slate\recovery\<pid>-<instante>\`.
Son JSON **en texto plano**, con texto, ruta original, huella base y cursor del
checkpoint. No constituyen un archivo histórico/versionado ni un respaldo cifrado.
En Unix se usan directorios privados y temporales con permisos restrictivos.

Cada buffer conserva la identidad de su registro al intercambiar o promover el
preview. Así no se elimina un borrador para trasladarlo a otro slot aún no durable.
Las sesiones simultáneas tienen directorios separados; no se ofrecen borradores
de procesos vivos. Un error de checkpoint se informa y permite reintentar.

## Recuperación

Al iniciar, Slate ofrece registros válidos de procesos que ya no están vivos.
↑/↓ seleccionan; **Enter** restaura como buffer principal modificado. No se
escribe automáticamente el archivo original. Es necesario guardar los buffers
actuales antes de sustituirlos con otro borrador recuperado.

**Esc / Later** mantiene los registros en disco para el próximo inicio. Los demás
borradores no seleccionados también permanecen allí. El registro recuperado no
se retira hasta que exista un nuevo checkpoint durable; guardar o descartar
explícitamente retira el borrador correspondiente. JSON corruptos/desconocidos
se conservan, se omiten en el selector y se registran en el log, no se borran.

El cierre normal pide confirmar los cambios del principal y del preview. Scratch
mantiene su comportamiento de archivado; si un archivado queda incompleto, no se
marca todo el texto como archivado ni se elimina el checkpoint correspondiente.
El cierre solicita un flush de checkpoints con espera limitada a dos segundos.

## Límites explícitos

- Un crash puede perder las teclas posteriores al último checkpoint terminado.
  No hay promesa de recuperar la última tecla ni de resistencia absoluta a cortes
  eléctricos, fallo del disco o del sistema de archivos.
- La comparación es optimista: sigue existiendo una pequeña ventana entre la
  última comprobación y el reemplazo. Un escritor externo que no colabora puede
  competir allí; no se promete exclusión mutua universal.
- La vigilancia detecta cambios de **contenido**, no cambios exclusivos de
  permisos/fecha; la elegibilidad del archivo se vuelve a comprobar al guardar.
- PID reutilizados o procesos inaccesibles pueden posponer la oferta de un registro
  antiguo. Es preferible eso a interferir con una sesión posiblemente viva.
- El guardado explícito sigue siendo síncrono y hace dos lecturas de verificación
  más sincronización. Documentos grandes/unidades remotas deben medirse en el
  punto 2 del roadmap. No se atribuye aquí ninguna mejora medida de rendimiento.
- Este alcance no convierte todas las escrituras auxiliares, configuración,
  archivado Scratch o modificaciones Flow en un sistema transaccional global.
  Un archivado parcial mantiene el texto, pero repetirlo puede duplicar las
  acciones parciales que sí se completaron.

## Validación

`cargo test --locked`: 188 pruebas, incluidas las del ajuste visual de avisos.
La implementación inicial pasó 185 con PATH reducido a Windows System32 (sin CLI SQLite).
`cargo build --release --locked` genera `target/release/slate.exe`.
La huella de apertura tiene un span separado `fingerprint_worker` en el log.
Incluye Unicode, creación sin clobber, modificación/eliminación externa, cambios
introducidos durante staging, temporales limpiados, readonly/hard links, fallo y
reintento del checkpoint, payload pendiente acotado, retiro posterior a durabilidad,
registros corruptos retenidos, sesiones vivas excluidas, tres buffers recuperados
tras salida abrupta de un proceso de prueba y preservación durante swaps/promoción.
Las pruebas usan documentos y configuración temporales propios, no documentos del usuario.

Los avisos de recuperación, cambios externos y confirmación de guardado comparten
un único panel centrado al 80%, tipografía monoespaciada, header/footer fijos y
acciones de texto clicables con sus atajos. La lista de recuperación selecciona
filas de ancho completo; sólo se desplaza el contenido. Las pruebas verifican
geometría a cuatro tamaños, selección visible y conservación del texto al volver.

Retest GUI recomendado: editar una copia de prueba, esperar ~2 segundos, cerrar
su proceso sin guardar y reabrir; restaurar y verificar sin tocar el original.
Modificar otra copia desde un editor externo y probar Keep text / Save copy / Reload.
