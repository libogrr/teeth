# Teeth

Teeth es una aplicación mínima para Windows 10 y 11 de 64 bits que graba el audio que reproducen las aplicaciones del PC en un archivo WAV idéntico a lo que entregan, antes del volumen de Windows. Graba todo el sistema excepto la propia Teeth, o una sola aplicación, nunca el micrófono. Permite recortar la toma con precisión de muestra y arrastrar el tramo elegido directamente a un DAW.

## Descarga y uso

Teeth se instala con `Teeth-Setup-1.0.1.exe`, que se descarga de la [última versión publicada](https://github.com/libogrr/teeth/releases/latest), en el apartado *Assets*. El instalador no pide permisos de administrador: instala Teeth solo para tu usuario, en `%LOCALAPPDATA%\Programs\Teeth`, y añade un acceso en el menú Inicio; el del escritorio es opcional. El ejecutable no está firmado, así que Windows puede avisar de que el editor es desconocido: pulsa «Más información» y después «Ejecutar de todas formas». Para desinstalar Teeth, usa Configuración → Aplicaciones. Tus samples, tus ajustes y las tomas sin guardar se quedan donde están.

Al abrirse, Teeth muestra su logo parpadeando despacio sobre negro durante unos tres segundos, mientras arranca por debajo. Un clic o una tecla lo saltan.

La barra de abajo tiene, de izquierda a derecha:

- **La fuente:** qué se graba. Al pulsarla se abre una lista con «Todo el sistema» y las aplicaciones que tienen audio; las que están sonando llevan unas barritas. Teeth recuerda la elección y la usa al abrirse si esa aplicación está abierta.
- **Al inicio (⏮):** lleva el cursor a donde empieza a sonar la selección: su principio o, con la reversa, su final. Si está sonando, vuelve a empezar desde ahí.
- **Grabar o parar (● / ■):** empieza o termina la grabación. Windows tarda un momento en empezar a capturar: mientras tanto se ve «Preparando…» y aún no se graba.
- **Escuchar (▶):** reproduce la selección actual de la toma.
- **Bucle (🔁):** se activa y se desactiva; activado queda en blanco. Mientras está activo, la selección se repite sin ningún hueco hasta que pausas. Se puede activar o desactivar mientras suena, y no se recuerda al cerrar Teeth.
- **El tiempo:** la posición del cursor y la duración de la toma.
- **Descargar (⤓):** guarda la selección en `Documentos\Teeth\Samples` con un clic, igual que al arrastrar la pestaña, y avisa con el nombre del archivo.
- **Tiradores:** en la fila que hay encima de la barra, a la derecha de los avisos, hay dos iconos para dejar limpios los bordes del tramo. Cruce por cero (`Z`) lleva los dos tiradores, como mucho 10 ms, a donde la onda cruza cero, para que el corte no suene como un clic. Recortar silencios (`T`) los acerca al primer y al último sonido de la selección; es silencio lo que está por debajo de −60 dBFS en todos los canales. Solo mueven los tiradores: el audio no cambia.
- **Variantes:** a continuación, en la misma fila, hay cuatro controles: bajar un semitono (`−`), la etiqueta de semitonos, subir un semitono (`+`) y `rev`. Cambian el tramo como lo haría una cinta: de −12 a +12 semitonos (más grave es también más lento, y más agudo, más rápido) y la reversa, solas o juntas. Un clic en la etiqueta (`0 st`) vuelve al tono original, y `rev` se activa y se desactiva. Se aplican al escuchar, al arrastrar la pestaña y al descargar, y el nombre del sample lo dice, por ejemplo `Google Chrome 2026-11-01 14-32-05 -3st rev.wav`. Ninguna variante recalcula muestras: la reversa solo reordena cuadros enteros y el pitch solo cambia la frecuencia que declara la cabecera. La toma no cambia y «Guardar como» (Ctrl+S) sigue dando siempre el audio fiel. Las teclas son `−` y `+` (también las del teclado numérico) y `I` para la reversa. Cada toma nueva empieza sin variante, y no se recuerda al cerrar la aplicación.
- **Descartar (✕):** cierra la toma. Si no está guardada, pide confirmación antes de borrarla.

Al inicio, Bucle, Descargar y las variantes solo aparecen cuando hay una toma abierta.

En la forma de onda:

- **Arrastrar sobre la onda** marca un tramo nuevo.
- **Arrastrar un tirador** mueve ese borde; con el zoom máximo se mueve de muestra en muestra.
- **Un clic** coloca el cursor de escucha.
- **La pestaña** que hay encima del tramo seleccionado muestra su duración; con una variante, la del archivo que saldrá. Arrástrala a Ableton Live, FL Studio, una carpeta o el escritorio para llevarte el tramo como archivo WAV. Si la pestaña lleva un punto rojo, el tramo pasa de 0 dBFS.
- **Las líneas ámbar** marcan los cortes: sitios donde Windows no entregó audio (ver «Cortes»).

Cada tramo arrastrado o descargado se guarda en `Documentos\Teeth\Samples` con el nombre de la fuente y la hora, por ejemplo `Google Chrome 2026-11-01 14-32-05.wav`. Los DAW enlazan ese archivo en vez de copiarlo, así que Teeth nunca borra nada de esa carpeta. Arrastrar o descargar dos veces el mismo tramo con la misma variante reutiliza el archivo. Si descargas la toma entera, o la sueltas entera en otro programa o carpeta, cuenta como guardada, salvo que lleve una variante: un sample con variante no es la toma, y descartarla sin guardar sigue pidiendo confirmación. Algunos antivirus (por ejemplo, el escudo contra ransomware de Avast) protegen `Documentos` y preguntan la primera vez si Teeth puede escribir ahí; hasta que se responde, la ventana espera.

Los atajos solo funcionan con la ventana activa:

| Tecla | Acción |
|---|---|
| R | Grabar o parar |
| Espacio | Escuchar o pausar |
| Inicio | Llevar el cursor a donde empieza a sonar la selección (su principio o, en reversa, su final) |
| L | Activar o desactivar el bucle |
| − / + | Bajar o subir un semitono (también con el teclado numérico) |
| I | Activar o desactivar la reversa |
| Z | Llevar los dos tiradores al cruce por cero más cercano |
| T | Recortar los silencios de la selección |
| Ctrl+S | Guardar como (elegir nombre y carpeta) |
| Supr | Descartar |
| ← → | Mover el tirador activo una muestra |
| Shift + ← → | Mover el tirador activo 10 ms |
| Rueda | Zoom |
| Shift + rueda | Desplazar |
| Barra bajo la onda (con zoom) | Pulsarla o arrastrarla para moverse por la toma |
| 0 | Ver la toma completa |

Para trabajar con un WAV que ya tienes, suéltalo en cualquier punto de la ventana. Se abre como una toma, con los mismos tiradores y variantes, y sus samples llevan su nombre. El archivo original no se toca ni se borra, así que cerrarlo no pregunta nada; solo «Guardar como» puede escribir encima, y Windows avisa antes. Si la toma abierta está sin guardar, pregunta antes de descartarla. Mientras se graba, no se puede abrir un archivo.

Mientras se graba, la toma se escribe en `%TEMP%\Teeth`. Si la aplicación se cierra de golpe, al abrirla de nuevo recupera la toma sin guardar.

Teeth funciona en una sola ventana, de 780 × 300 como mínimo para que quepa la barra, y la abre donde la dejaste y con el mismo tamaño. Si ya está abierta, abrirla otra vez la trae al frente, aunque esté minimizada.

## Qué significa «fiel»

Teeth sigue siete reglas, que mandan sobre cualquier otra decisión de diseño:

1. Los bytes de audio del archivo son los mismos bytes que entrega WASAPI. Ningún código los transforma.
2. El formato del archivo es el formato de mezcla del dispositivo: la misma frecuencia, la misma profundidad, el mismo tipo de muestra y el mismo número de canales.
3. El archivo contiene solo los bloques que exige el formato WAV. No se escribe ninguna información adicional.
4. La duración del archivo coincide con el tiempo real de la grabación. Los silencios se graban como ceros.
5. Si se pierde audio, la app lo dice. Nunca rellena ni disimula un corte.
6. El recorte copia un rango de bytes. No recalcula ninguna muestra.
7. Una variante (la reversa o el pitch de cinta) solo reordena cuadros enteros o cambia la frecuencia que declara la cabecera, y nunca recalcula muestras: los bytes de cada cuadro siguen siendo los que entregó WASAPI. Se aplica solo al escuchar, al arrastrar la pestaña y al descargar. La toma no cambia, «Guardar como» da el audio fiel y un sample con variante no cuenta como guardar la toma.

Fiel quiere decir fiel a lo que reproducen las aplicaciones. Teeth graba antes del volumen de Windows: con el volumen al 14 % o al 100 %, el archivo es el mismo. El volumen solo cambia lo que oyes.

Lo que sí cambia la toma es el volumen propio de cada aplicación: el control de la aplicación en el mezclador de volumen de Windows y, en muchas aplicaciones, su propio deslizador de volumen (por ejemplo el de TIDAL). Para que la toma sea igual a lo que entrega la aplicación, deja esos dos controles al 100 % y regula lo que oyes con el volumen de Windows. Medido en este equipo: una aplicación con su control al 50 % se graba 12,04 dB más baja. Si una mejora de audio del controlador modifica la señal antes del punto de captura, la toma refleja esa modificación; la prueba de fidelidad lo comprueba en cada equipo.

### Todo el sistema excepto Teeth

Teeth graba todo lo que reproducen las aplicaciones del PC menos ella misma, así que nunca graba su propia reproducción. El archivo usa el formato de mezcla del dispositivo de salida por defecto.

Los sonidos de las demás aplicaciones también entran en la toma: las notificaciones de Windows, el navegador, un chat o un vídeo en otra pestaña se mezclan con lo que quieres grabar. Para una toma limpia, elige en la fuente solo la aplicación que quieres grabar, o cierra o silencia las demás.

### Cortes

Cuando se graba todo el sistema, cada vez que una aplicación abre o cierra un sonido (una notificación, por ejemplo), Windows pierde entre 33 y 55 ms del audio que se estaba capturando, sin avisar. Teeth lo detecta por la marca de tiempo de cada paquete y lo señala con una línea ámbar en la onda. Ese audio no se puede recuperar: si el corte cae en el tramo que quieres, repite la toma.

Grabando una sola aplicación, los sonidos de las demás no producen cortes. La propia aplicación sí puede producirlos si abre o cierra su sonido, por ejemplo al pausar o saltar en un vídeo.

### Versiones de Windows sin captura antes del volumen

Para grabar antes del volumen, Teeth usa la captura de audio por aplicación de Windows. Windows 11 la incluye; en Windows 10 depende de la versión. Si no está disponible, Teeth graba la salida del dispositivo por defecto y muestra el aviso «Esta versión de Windows no permite grabar antes del volumen: la toma depende del volumen de Windows.». En ese caso, en algunos equipos la toma sale más baja cuanto más bajo está el volumen de Windows; para una toma fiel, deja el volumen de Windows al 100 %.

## Prueba de fidelidad

El binario `fidelity` comprueba la cadena completa con el dispositivo real. Reproduce por WASAPI, en el formato de mezcla, 0,5 s de silencio, 5 s de ruido pseudoaleatorio de semilla fija y amplitud 0,1 y otros 0,3 s de silencio, mientras el grabador de Teeth captura el audio de la propia herramienta. Después busca en la grabación el punto donde las primeras 64 muestras coinciden bit a bit con la señal y compara los 5 s de ruido completos. El silencio final solo sirve para que el final del ruido llegue a la grabación; no se compara.

Para ejecutarla:

1. Desactiva las mejoras de audio del dispositivo de salida.
2. Baja el volumen físico de los altavoces o usa auriculares, porque suena ruido.
3. Ejecuta `cargo run --release --bin fidelity`. El volumen de Windows puede estar en cualquier nivel; para comprobar que no influye, repite la prueba con otro nivel y compara los resultados.

La herramienta indica primero qué captura obtuvo: `Captura por aplicación: independiente del volumen de Windows.` o, en las versiones de Windows sin esa captura, `Captura del dispositivo: depende del volumen de Windows.`. Termina con código 0 e imprime `BIT A BIT: N de N muestras idénticas` si todas las muestras coinciden. Si no, termina con código 1 e imprime `NO ES BIT A BIT: ganancia aparente X dB, diferencia máxima Y`. Solo admite mezclas en coma flotante de 32 bits; con otro formato imprime `Formato no compatible con la prueba` y termina con código 2. La herramienta borra su grabación temporal al terminar.

Resultado (volumen de Windows al 14 %): BIT A BIT, 480000 de 480000 muestras idénticas (prueba de captura por aplicación).

Resultado con el modo que usa la app, todo el sistema excepto Teeth (volumen de Windows al 14 %, sin otro audio sonando): BIT A BIT, 480000 de 480000 muestras idénticas.

Resultado del 7 de noviembre de 2026, con Teeth 1.0.0: BIT A BIT, 480000 de 480000 muestras idénticas.

## Limitaciones conocidas

Guardar o recuperar una toma muy larga, de varias horas, puede dejar la ventana sin responder unos segundos mientras se copia o se analiza el archivo.

## Compilación

Requiere Rust estable con el destino `x86_64-pc-windows-msvc`.

```
cargo build --release
```

Los ejecutables quedan en `target\release`: `teeth.exe` y `fidelity.exe`. Para probar el código que no necesita dispositivo de audio, ejecuta `cargo test`.

Para construir el instalador hace falta también Inno Setup 6.7 o posterior, que se instala sin permisos de administrador con `winget install JRSoftware.InnoSetup --scope user`. Después:

```
powershell -ExecutionPolicy Bypass -File tools\build_installer.ps1
```

Compila en release y deja `C:\t\instalador\Teeth-Setup-<versión>.exe`.

El icono sale del kit del logo. `tools/make_icon.py` (con Pillow) genera, a partir de `assets/icon/teeth.png`, la silueta minimalista, el `.ico` que `build.rs` incrusta en el ejecutable, el icono de la ventana y la silueta de la pantalla de inicio. `tools/make_installer_art.py` genera las imágenes del asistente a partir de esa silueta, así que se ejecuta después de `make_icon.py`. Los resultados están en el repositorio, así que compilar no necesita Python.

Si la ruta del proyecto es muy larga, el enlazador puede fallar con el error `LNK1104`. En ese caso define `CARGO_TARGET_DIR` con una ruta corta antes de compilar, por ejemplo:

```
set CARGO_TARGET_DIR=C:\t\teeth
cargo build --release
```

La interfaz usa egui con eframe y el renderizador glow.

## Licencia

El código de Teeth se publica con la licencia MIT; el texto completo está en `LICENSE`.

El logo de Teeth, «Cuernos», no entra en esa licencia. El logo y las imágenes que salen de él (`assets/icon/` e `installer/art/`) tienen todos los derechos reservados: no se pueden usar fuera de Teeth sin permiso. Quien publique una versión modificada debe cambiarle el nombre y el logo.

## Créditos

El logo de Teeth, «Cuernos», es obra del autor del proyecto.

La interfaz usa la fuente [Inter](https://github.com/rsms/inter) (Copyright 2016 The Inter Project Authors), publicada con la SIL Open Font License 1.1. La licencia completa está en `assets/fonts/OFL.txt`.

Teeth reutiliza ideas de [ONCExDEV/system-audio-recorder](https://github.com/ONCExDEV/system-audio-recorder), publicado con licencia MIT: el flujo de reproducción silencioso que mantiene vivo el loopback y la idea de detectar los cambios de dispositivo de salida. El código derivado conserva el aviso de copyright y la licencia MIT del original:

```
MIT License

Copyright (c) 2026 System Audio Recorder contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
