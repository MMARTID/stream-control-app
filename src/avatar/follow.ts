// Cuándo seguir la cara: solo mientras la escena del avatar está en pantalla.
// Así la cámara queda libre para OBS en las demás escenas y no se gasta CPU ni GPU de más.

export interface FollowInput {
  /** El usuario ha elegido «seguir solo con la escena del avatar». */
  follow: boolean;
  /** El avatar está activado y su servidor funciona. */
  ready: boolean;
  /** La escena en programa de OBS es la del avatar. */
  onAir: boolean;
  /** Hay un seguimiento en marcha. */
  active: boolean;
  /** El seguimiento en marcha lo arrancó el automático (no el botón «Probar»). */
  startedByFollow: boolean;
  /** Se acaba de parar a mano o falló: no reintentar hasta cambiar de escena. */
  suppressed: boolean;
  /** Se está arrancando ahora mismo. */
  starting: boolean;
}

export type FollowAction = "start" | "stop" | "none";

export function decideFollow(i: FollowInput): FollowAction {
  const mine = i.active && i.startedByFollow;
  if (!i.follow || !i.ready) return mine ? "stop" : "none";
  if (i.onAir) return !i.active && !i.starting && !i.suppressed ? "start" : "none";
  return mine ? "stop" : "none";
}
