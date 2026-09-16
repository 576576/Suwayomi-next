//! rx.Observable -> suspend bridge (matches the server's awaitSingle helper).
package suwayomi.tachidesk.manga.impl.util.lang

import kotlinx.coroutines.suspendCancellableCoroutine
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

suspend fun <T> rx.Observable<T>.awaitSingle(): T = suspendCancellableCoroutine { cont ->
    val subscription = subscribe(
        { cont.resume(it) },
        { cont.resumeWithException(it) },
    )
    cont.invokeOnCancellation { subscription.unsubscribe() }
}

/** Empty JSON object constant used by the model impls. */
val EMPTY: kotlinx.serialization.json.JsonObject = kotlinx.serialization.json.JsonObject(emptyMap())
