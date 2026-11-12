package org.krunk.browser.ui.browser

import android.app.PendingIntent
import android.content.ActivityNotFoundException
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.text.format.DateFormat
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.TimePickerDialog
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import java.io.File
import java.time.LocalDate
import java.time.LocalTime
import java.time.Month
import java.time.YearMonth
import java.time.format.TextStyle
import java.util.concurrent.atomic.AtomicBoolean
import org.krunk.browser.R
import org.krunk.browser.engine.DateKind
import org.krunk.browser.engine.DateValues
import org.krunk.browser.engine.PagePrompt
import org.krunk.browser.engine.Shares
import org.krunk.browser.engine.Uploads

/** The asking site for a title, or "This page" when it has none (a local file, `about:blank`). */
@Composable
internal fun askingSite(prompt: PagePrompt): String = prompt.site.ifEmpty { stringResource(R.string.prompt_this_page) }

// ----- file field -----

/**
 * A file field: Android's document picker, opened at once (the user has just tapped the
 * field); it needs no permission and shows only what the page accepts. A field that asks for a
 * fresh photo first gets a question, titled with the site: camera or a file. Closing the
 * picker or the camera without a result gives the page no file.
 */
@Composable
internal fun FilePromptUi(prompt: PagePrompt.File) {
    val context = LocalContext.current
    var started by remember { mutableStateOf(prompt.launched) }
    var photo by rememberSaveable { mutableStateOf<String?>(null) }
    val one = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        prompt.answer(listOfNotNull(uri?.toString()))
    }
    val many = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        prompt.answer(uris.map { it.toString() })
    }
    val camera = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { saved ->
        val file = photo?.let { File(it) }
        if (saved && file != null) {
            prompt.answer(listOf(Uri.fromFile(file).toString()))
        } else {
            file?.delete()
            prompt.dismiss()
        }
    }

    fun pick() {
        prompt.launched = true
        started = true
        val types = prompt.request.types.toTypedArray()
        try {
            if (prompt.request.multiple) many.launch(types) else one.launch(types)
        } catch (_: ActivityNotFoundException) {
            prompt.dismiss()
        }
    }

    fun shoot() {
        prompt.launched = true
        started = true
        val (file, address) = Uploads.newCapture(context)
        photo = file.path
        try {
            camera.launch(address)
        } catch (_: ActivityNotFoundException) {
            // No camera app: the picker is the next best thing.
            file.delete()
            pick()
        }
    }

    if (!prompt.request.camera) {
        LaunchedEffect(prompt) { if (!prompt.launched) pick() }
    } else if (!started) {
        AlertDialog(
            onDismissRequest = prompt::dismiss,
            title = { Text(stringResource(R.string.prompt_photo_title, askingSite(prompt))) },
            text = {
                Column {
                    TextButton(onClick = ::shoot) { Text(stringResource(R.string.prompt_photo_take)) }
                    TextButton(onClick = ::pick) { Text(stringResource(R.string.prompt_photo_choose)) }
                }
            },
            confirmButton = {},
            dismissButton = {
                TextButton(onClick = prompt::dismiss) { Text(stringResource(android.R.string.cancel)) }
            },
        )
    }
}

// ----- navigator.share -----

private const val ACTION_SHARE_PICKED = "org.krunk.browser.SHARE_PICKED"

/** Android tells which app was picked just after the sheet has closed: wait this long for it. */
private const val PICK_GRACE_MS = 700L

/**
 * `navigator.share()`: Android's share sheet with the page's text and address. The sheet names
 * the site in its title where the system shows one. See [Shares.outcome] for the answer.
 */
@Composable
internal fun SharePromptUi(prompt: PagePrompt.Share) {
    val context = LocalContext.current
    val picked = remember(prompt) { AtomicBoolean(false) }
    val sheet = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        // Not tied to this composition: the page must get its answer even if the screen changes.
        Handler(Looper.getMainLooper()).postDelayed(
            { prompt.result(Shares.outcome(started = true, picked = picked.get())) },
            PICK_GRACE_MS,
        )
    }
    DisposableEffect(prompt) {
        val receiver = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) = picked.set(true)
        }
        ContextCompat.registerReceiver(
            context,
            receiver,
            IntentFilter(ACTION_SHARE_PICKED),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
        onDispose { context.unregisterReceiver(receiver) }
    }
    val title = stringResource(R.string.prompt_share_title, askingSite(prompt))
    LaunchedEffect(prompt) {
        if (prompt.launched) return@LaunchedEffect
        prompt.launched = true
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, prompt.text)
            if (prompt.title.isNotEmpty()) {
                putExtra(Intent.EXTRA_SUBJECT, prompt.title)
                putExtra(Intent.EXTRA_TITLE, prompt.title)
            }
        }
        // The system adds the picked app to this broadcast, so it must be changeable; it can
        // only reach this app.
        val mutable = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0
        val callback = PendingIntent.getBroadcast(
            context,
            0,
            Intent(ACTION_SHARE_PICKED).setPackage(context.packageName),
            PendingIntent.FLAG_UPDATE_CURRENT or mutable,
        )
        try {
            sheet.launch(Intent.createChooser(send, title, callback.intentSender))
        } catch (_: ActivityNotFoundException) {
            prompt.result(Shares.outcome(started = false, picked = false))
        }
    }
}

// ----- leave page, send again -----

@Composable
internal fun LeavePageDialog(prompt: PagePrompt.LeavePage) {
    AlertDialog(
        onDismissRequest = prompt::dismiss,
        title = {
            Text(
                if (prompt.site.isEmpty()) {
                    stringResource(R.string.prompt_leave_title)
                } else {
                    stringResource(R.string.prompt_leave_title_site, prompt.site)
                },
            )
        },
        text = { Text(stringResource(R.string.prompt_leave_text)) },
        confirmButton = {
            TextButton(onClick = { prompt.answer(true) }) { Text(stringResource(R.string.prompt_leave)) }
        },
        dismissButton = {
            TextButton(onClick = { prompt.answer(false) }) { Text(stringResource(R.string.prompt_stay)) }
        },
    )
}

@Composable
internal fun RepostDialog(prompt: PagePrompt.Repost) {
    AlertDialog(
        onDismissRequest = prompt::dismiss,
        title = { Text(stringResource(R.string.prompt_repost_title)) },
        text = {
            Text(
                if (prompt.site.isEmpty()) {
                    stringResource(R.string.prompt_repost_text_page)
                } else {
                    stringResource(R.string.prompt_repost_text, prompt.site)
                },
            )
        },
        confirmButton = {
            TextButton(onClick = { prompt.answer(true) }) { Text(stringResource(R.string.prompt_repost_send)) }
        },
        dismissButton = {
            TextButton(onClick = { prompt.answer(false) }) { Text(stringResource(android.R.string.cancel)) }
        },
    )
}

// ----- date and time fields -----

/**
 * A date or time field. "OK" answers in the field's format, "Clear" empties the field, and
 * "Cancel", Back or a tap outside leave it as it was. Days outside the field's `min`..`max`
 * cannot be picked; for times the page's own check applies.
 */
@Composable
internal fun DateTimePromptDialog(prompt: PagePrompt.DateTime) {
    val site = askingSite(prompt)
    when (prompt.kind) {
        DateKind.DATE -> DayDialog(prompt, stringResource(R.string.prompt_date_title, site), last = true) {
            prompt.answer(DateValues.date(it))
        }

        DateKind.WEEK -> DayDialog(prompt, stringResource(R.string.prompt_week_title, site), last = true) {
            prompt.answer(DateValues.week(it))
        }

        DateKind.TIME -> ClockDialog(
            prompt,
            stringResource(R.string.prompt_time_title, site),
            DateValues.parseTime(prompt.value),
        ) { prompt.answer(DateValues.time(it)) }

        DateKind.DATE_TIME -> {
            // Two steps: the day, then the time of day.
            var day by remember { mutableStateOf<LocalDate?>(null) }
            val picked = day
            if (picked == null) {
                DayDialog(prompt, stringResource(R.string.prompt_date_title, site), last = false) { day = it }
            } else {
                ClockDialog(
                    prompt,
                    stringResource(R.string.prompt_time_title, site),
                    DateValues.parseDateTime(prompt.value)?.toLocalTime(),
                ) { prompt.answer(DateValues.dateTime(picked, it)) }
            }
        }

        DateKind.MONTH -> MonthDialog(prompt, stringResource(R.string.prompt_month_title, site))
    }
}

@Composable
private fun ClearAndCancel(prompt: PagePrompt.DateTime) {
    Row {
        TextButton(onClick = prompt::clear) { Text(stringResource(R.string.prompt_clear)) }
        TextButton(onClick = prompt::dismiss) { Text(stringResource(android.R.string.cancel)) }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DayDialog(prompt: PagePrompt.DateTime, title: String, last: Boolean, onPick: (LocalDate) -> Unit) {
    val kind = prompt.kind
    val start = remember(prompt) { DateValues.startDay(kind, prompt.value, prompt.min, prompt.max, LocalDate.now()) }
    val state = rememberDatePickerState(
        initialSelectedDateMillis = DateValues.dayOf(kind, prompt.value)?.let(DateValues::utcMillis),
        initialDisplayedMonthMillis = DateValues.utcMillis(start),
        yearRange = DateValues.years(kind, prompt.value, prompt.min, prompt.max),
        selectableDates = object : SelectableDates {
            override fun isSelectableDate(utcTimeMillis: Long): Boolean =
                DateValues.allowed(kind, DateValues.fromUtcMillis(utcTimeMillis), prompt.min, prompt.max)
        },
    )
    val selected = state.selectedDateMillis?.let(DateValues::fromUtcMillis)
    DatePickerDialog(
        onDismissRequest = prompt::dismiss,
        confirmButton = {
            TextButton(enabled = selected != null, onClick = { selected?.let(onPick) }) {
                Text(stringResource(if (last) android.R.string.ok else R.string.prompt_next))
            }
        },
        dismissButton = { ClearAndCancel(prompt) },
    ) {
        DatePicker(
            state = state,
            title = {
                Text(title, modifier = Modifier.padding(start = 24.dp, end = 12.dp, top = 16.dp))
            },
            headline = if (kind == DateKind.WEEK && selected != null) {
                {
                    val (week, year) = DateValues.weekOf(selected)
                    Text(
                        stringResource(R.string.prompt_week_label, week, year),
                        modifier = Modifier.padding(start = 24.dp, end = 12.dp, bottom = 12.dp),
                    )
                }
            } else {
                null
            },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ClockDialog(prompt: PagePrompt.DateTime, title: String, initial: LocalTime?, onPick: (LocalTime) -> Unit) {
    val context = LocalContext.current
    val now = remember(prompt) { initial ?: LocalTime.now() }
    val state = rememberTimePickerState(
        initialHour = now.hour,
        initialMinute = now.minute,
        is24Hour = DateFormat.is24HourFormat(context),
    )
    TimePickerDialog(
        onDismissRequest = prompt::dismiss,
        title = { Text(title) },
        confirmButton = {
            TextButton(onClick = { onPick(LocalTime.of(state.hour, state.minute)) }) {
                Text(stringResource(android.R.string.ok))
            }
        },
        dismissButton = { ClearAndCancel(prompt) },
    ) {
        TimePicker(state = state)
    }
}

private const val MONTHS_PER_ROW = 3

@Composable
private fun MonthDialog(prompt: PagePrompt.DateTime, title: String) {
    val start = remember(prompt) {
        DateValues.startDay(DateKind.MONTH, prompt.value, prompt.min, prompt.max, LocalDate.now())
    }
    val years = remember(prompt) { DateValues.years(DateKind.MONTH, prompt.value, prompt.min, prompt.max) }
    var year by remember { mutableIntStateOf(start.year) }
    var month by remember { mutableStateOf(DateValues.parseMonth(prompt.value)?.monthValue) }

    val locale = LocalConfiguration.current.locales[0]

    fun allowed(m: Int) = DateValues.allowed(DateKind.MONTH, LocalDate.of(year, m, 1), prompt.min, prompt.max)
    val chosen = month?.takeIf(::allowed)
    AlertDialog(
        onDismissRequest = prompt::dismiss,
        title = { Text(title) },
        text = {
            Column {
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    IconButton(enabled = year > years.first, onClick = { year-- }) {
                        Icon(
                            Icons.AutoMirrored.Filled.KeyboardArrowLeft,
                            contentDescription = stringResource(R.string.prompt_previous_year),
                        )
                    }
                    Text(year.toString(), style = MaterialTheme.typography.titleMedium)
                    IconButton(enabled = year < years.last, onClick = { year++ }) {
                        Icon(
                            Icons.AutoMirrored.Filled.KeyboardArrowRight,
                            contentDescription = stringResource(R.string.prompt_next_year),
                        )
                    }
                }
                (1..Month.DECEMBER.value).chunked(MONTHS_PER_ROW).forEach { row ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
                        row.forEach { m ->
                            FilterChip(
                                selected = month == m,
                                enabled = allowed(m),
                                onClick = { month = m },
                                label = { Text(Month.of(m).getDisplayName(TextStyle.SHORT, locale)) },
                            )
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = chosen != null,
                onClick = { chosen?.let { prompt.answer(DateValues.month(YearMonth.of(year, it))) } },
            ) { Text(stringResource(android.R.string.ok)) }
        },
        dismissButton = { ClearAndCancel(prompt) },
    )
}
