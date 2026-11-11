package org.krunk.browser.ui.newtab

import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import org.krunk.browser.tabs.Favicons
import org.krunk.ffi.hostOf

/**
 * A site's icon from the favicon cache (the core `favicons` table, filled when the user visited
 * the site), or a letter on a colour derived from the host. Nothing is fetched here.
 */
@Composable
fun SiteIcon(
    url: String,
    favicons: Favicons,
    size: Dp = 32.dp,
    modifier: Modifier = Modifier,
    fallbackText: String? = null,
) {
    val host = remember(url) { hostOf(url) }
    var bitmap by remember(host) { mutableStateOf<Bitmap?>(null) }
    LaunchedEffect(host) {
        bitmap = host?.let { favicons.load(it) }
    }
    val shape = RoundedCornerShape(size / 4)
    val bmp = bitmap
    if (bmp != null) {
        Image(
            bitmap = bmp.asImageBitmap(),
            contentDescription = null,
            modifier = modifier.size(size).clip(shape),
        )
    } else {
        val letter = (fallbackText?.trim()?.firstOrNull() ?: host?.removePrefix("www.")?.firstOrNull() ?: '?')
            .uppercaseChar()
            .toString()
        Box(
            modifier = modifier.size(size).clip(shape).background(letterColor(host ?: url)),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                letter,
                color = Color.White,
                fontWeight = FontWeight.SemiBold,
                fontSize = (size.value * 0.5f).sp,
                style = MaterialTheme.typography.titleMedium,
            )
        }
    }
}

/** A stable, muted colour for a host (same host, same colour). */
fun letterColor(key: String): Color {
    val palette = listOf(
        Color(0xFF2563EB),
        Color(0xFF7C3AED),
        Color(0xFFDB2777),
        Color(0xFFEA580C),
        Color(0xFF059669),
        Color(0xFF0891B2),
        Color(0xFF4F46E5),
        Color(0xFFB45309),
    )
    var h = 0
    for (c in key) h = h * 31 + c.code
    return palette[Math.floorMod(h, palette.size)]
}
