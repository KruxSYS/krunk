package org.krunk.browser.core

import android.content.res.AssetManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.LruCache
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.ffi.Wallpaper
import org.krunk.ffi.wallpaper
import org.krunk.ffi.wallpapers

/**
 * The ten built-in new-tab wallpapers: metadata from the core (`ui/shared/wallpapers/
 * wallpapers.json`), pictures from the APK assets Gradle copies there (`assets/wallpapers/
 * <id>.webp` and `<id>-thumb.webp`). Decoded bitmaps are cached per process; nothing is fetched.
 */
object Wallpapers {
    const val ASSET_DIR = "wallpapers"

    // One full-size picture (2048x1152, about 9 MB decoded): only the chosen one is on screen.
    private val cache = LruCache<String, Bitmap>(1)
    private val thumbs = LruCache<String, Bitmap>(16)

    val all: List<Wallpaper> by lazy { wallpapers() }

    /** The wallpaper for a settings id, or `null` when the id is unknown (or empty). */
    fun find(id: String): Wallpaper? = if (id.isEmpty()) null else wallpaper(id)

    fun assetPath(id: String, thumb: Boolean = false): String =
        if (thumb) "$ASSET_DIR/$id-thumb.webp" else "$ASSET_DIR/$id.webp"

    /** Whether text drawn over the wallpaper should be light (dark picture) or dark. */
    fun lightText(w: Wallpaper): Boolean = w.look == "dark"

    suspend fun load(assets: AssetManager, id: String, io: CoroutineDispatcher = Dispatchers.IO): Bitmap? {
        if (find(id) == null) return null
        cache.get(id)?.let { return it }
        val bmp = withContext(io) { decode(assets, assetPath(id)) } ?: return null
        cache.put(id, bmp)
        return bmp
    }

    suspend fun loadThumb(assets: AssetManager, id: String, io: CoroutineDispatcher = Dispatchers.IO): Bitmap? {
        if (find(id) == null) return null
        thumbs.get(id)?.let { return it }
        val bmp = withContext(io) { decode(assets, assetPath(id, thumb = true)) } ?: return null
        thumbs.put(id, bmp)
        return bmp
    }

    private fun decode(assets: AssetManager, path: String): Bitmap? =
        runCatching { assets.open(path).use { BitmapFactory.decodeStream(it) } }.getOrNull()
}
