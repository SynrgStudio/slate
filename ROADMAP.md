# Slate — próximos cinco pasos

Orden acordado con el usuario. Preservar el estilo minimalista, el control por
teclado y la snappiness; no sacrificar texto del usuario por automatismos.

1. **Seguridad y recuperación del texto — implementado; estilo validado por el usuario**
   - Guardado atómico, sin truncar el destino antes de terminar la escritura.
   - Recuperar buffers sin guardar tras cierres inesperados, incluidos buffers
     nuevos y modales; aislar sesiones simultáneas.
   - Detectar cambios externos y evitar sobrescribirlos silenciosamente.
   - Validar fallos, concurrencia, recuperación y conservación del texto.
   - Entrega: guardado protegido principal/preview, checkpoints de los tres
     buffers, recuperación explícita y avisos externos; 188 pruebas pasan.
     Avisos unificados y ajuste visual aprobado; retest manual de recuperación
     tras cierre abrupto sigue recomendado.
     Alcance, limitaciones y retest: [docs/document-safety.md](docs/document-safety.md).
2. **Snappiness con documentos grandes — en curso; apertura/scroll/edición validados por el usuario**
   - Medir edición, búsqueda, wrapping y Markdown; optimizar sólo cuellos de
     botella demostrados.
   - Fixture aislada: 5.000.000 caracteres, 27.343 líneas; instrucciones y
     datos reproducibles en [docs/large-document-baseline.md](docs/large-document-baseline.md).
   - Wrapping lineal, filas reutilizadas y contexto Markdown indexado: benchmark
     aislado inicial 18,7 s → 65,8 ms; frame con scroll 1,34 s → 0,087 ms.
   - Edición incremental: insertar + frame 58,6 ms → 2–3 ms; undo + frame
     60 ms → 5–6 ms en CPU aislada. 197 pruebas pasan.
   - Retest GUI: el usuario editó y guardó una copia; 36 actualizaciones de
     layout recalcularon una sola línea (mediana 0,243 ms). Apertura hasta el
     siguiente frame de CPU: 184 ms. Pendiente: búsqueda/preview y trabajo Flow;
     guardado confirmado, aún sin medición de latencia.
   - Búsqueda confirmada fluida por el usuario, aún sin timing específico.
   - Scroll bilateral editor/preview por anclas y alturas reales implementado:
     funcionamiento aprobado por el usuario. El rendimiento de preview y
     trabajo Flow sigue por perfilar.
   - Periscope: Everything opcional por IPC en Windows, consultas coalescidas
     fuera de UI y respaldo SQLite persistente de metadatos, global en discos
     fijos locales en vez de Home. Prueba real de archivo fuera de Home: ~130 ms
     de consulta IPC. 231 pruebas pasan; readiness gate corregido tras reporte
     GUI de "IPC bloqueado", consulta real y errores Win32 separados. Retest GUI
     pendiente; sin cambios de permisos ni descarte del cache.
   - Primera apertura: consentimiento para precache SQLite de todos los roots,
     espera obligatoria con progreso, y sólo después elección proyecto/global.
     Importa metadatos QUERY2 de Everything por páginas; fallback sólo para roots
     sin inventario. 22.719 archivos importados a DB temporal en ~986 ms, sin stat
     por archivo. Preparación persistida por scope; cancelar/fallar no abre búsqueda.
     Importación tolera altas/bajas y páginas repetidas del índice vivo, sin
     reinicios: pasada acotada al total inicial, no snapshot transaccional.
     Resultados Periscope en frame común: viewport adaptativo, footer fijo,
     navegación por rueda, sin desborde. Ranking multipalabra/abreviaturas corregido;
     consulta real `Abrir minis` encuentra el CMD del Desktop (~1,31 s). Consultas
     vacías no escanean, búsquedas por nombre primero, timeout IPC de 5 s.
     Búsqueda nativa por palabras primero (sin regex), fuzzy como respaldo.
     HWND IPC reutilizado, tags descartan replies viejas, espera cancelable y
     coalescing de 40 ms. `abrir mi` ~41 ms / `mi abrir` ~47 ms en IPC real;
     no son medidas de latencia GUI end-to-end.
     Proyectos explícitos: Menú · Ctrl+P (texto abajo derecha, sin menú superior)
     abre la command palette; create-project asigna carpeta sin mover/guardar
     documento. Sin asociación Periscope va Global; con asociación ofrece scopes.
     Persistencia por carpeta/path, untitled por sesión y luego Save As; no hereda
     último scope ni Git. Metadata JSON con guard de conflicto/corrupción.
     260 pruebas pasan serialmente; retest GUI pendiente. Alcance/límites:
     [docs/periscope-index.md](docs/periscope-index.md).
   - Resize + preview: benchmark CPU por cambio de ancho 630–854 ms →
     16–17 ms, materializando contenido visible y estimando alturas offscreen.
     resize validado por el usuario. 211 pruebas pasan.
3. **Consistencia de las ventanas restantes — en curso**
   - Header/footer fijos, contenido desplazable, selección visible y geometría
     acotada en todos los diálogos restantes.
   - Primera tanda: prompt Periscope, Capture, Scratch y buffer modal usan
     un único marco con chrome fijo y contenido adaptable; 220 pruebas pasan.
     Capture/Scratch: apariencia aprobada; centrado corregido tras screenshots
     del usuario, con pruebas de posición y resize. Retest de centrado pendiente. Auditoría y cola exacta:
     [docs/dialog-consistency.md](docs/dialog-consistency.md).
   - Periscope y Find note migrados: 80%, chrome fijo, filas/columnas adaptables,
     wheel, selección visible y acciones clickeables. Find note conserva búsqueda,
     expansión de headings y guards de texto dirty; 266 pruebas pasan serialmente.
     Lecturas visibles de headings siguen síncronas: perf pendiente, sin claim de
     velocidad nuevo. Retest GUI Find note pendiente.
   - Próxima tanda: Link assist; luego Save As, Scratch entries y Flow/Agenda.
4. **Highlight de código liviano — pendiente**
   - Local e incremental; sin depender de un servidor LSP obligatorio.
5. **Descubribilidad y accesibilidad — pendiente**
   - Comandos, atajos, foco y contraste coherentes y fáciles de aprender.
   - Adelanto solicitado: footer compacto/contextual y diez casillas de
     visibilidad persistidas en Settings; apariencia y controles aprobados
     por el usuario: [docs/compact-footer.md](docs/compact-footer.md).
   - Ayudas de commandline adaptativas: se ocultan bloques completos en ventanas
     angostas; no se modifica el comando escrito. Helpers y titlebar aprobados
     por el usuario.

Registrar el alcance y las limitaciones de cada entrega; no declarar recuperación
sin pérdida de la última tecla ni protección absoluta contra escritores externos
que no colaboran sin evidencia de esas garantías.
