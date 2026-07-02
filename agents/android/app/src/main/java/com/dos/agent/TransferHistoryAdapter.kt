package com.dos.agent

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.ImageView
import android.widget.TextView
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView
import java.io.File
import java.text.SimpleDateFormat
import java.util.*

class TransferHistoryAdapter(
    private val onResend: ((TransferRecord) -> Unit)? = null
) : ListAdapter<TransferRecord, TransferHistoryAdapter.ViewHolder>(HistoryDiffCallback()) {

    private val dateFormat = SimpleDateFormat("MM/dd HH:mm", Locale.US)

    class ViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
        val thumbnail: ImageView = itemView.findViewById(R.id.historyThumbnail)
        val directionIcon: TextView = itemView.findViewById(R.id.historyDirection)
        val filename: TextView = itemView.findViewById(R.id.historyFilename)
        val size: TextView = itemView.findViewById(R.id.historySize)
        val speed: TextView = itemView.findViewById(R.id.historySpeed)
        val time: TextView = itemView.findViewById(R.id.historyTime)
        val status: TextView = itemView.findViewById(R.id.historyStatus)
        val resend: TextView = itemView.findViewById(R.id.historyResend)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.history_item, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val item = getItem(position)
        val ctx = holder.itemView.context

        holder.directionIcon.text = if (item.direction == TransferDirection.SEND) "⬆" else "⬇"
        holder.filename.text = item.filename
        holder.size.text = FileTransferManager.formatSize(item.totalBytes)
        holder.speed.text = "avg ${FileTransferManager.formatSpeed(item.averageSpeed)}"
        holder.time.text = dateFormat.format(Date(item.timestamp))

        val ext = item.filename.substringAfterLast('.', "").lowercase()
        if (ext in listOf("jpg", "jpeg", "png", "gif", "webp", "bmp")) {
            holder.thumbnail.visibility = View.VISIBLE
            try {
                val downloadDir = SettingsManager.getDownloadDir()
                val file = File(downloadDir, item.filename)
                if (file.exists()) {
                    val bm = BitmapFactory.decodeFile(file.absolutePath)
                    if (bm != null) {
                        val thumb = android.graphics.Bitmap.createScaledBitmap(bm, 64, 64, true)
                        holder.thumbnail.setImageBitmap(thumb)
                    }
                }
            } catch (_: Exception) {
                holder.thumbnail.visibility = View.GONE
            }
        } else {
            holder.thumbnail.visibility = View.GONE
        }

        holder.status.apply {
            when (item.status) {
                TransferStatus.COMPLETED -> {
                    text = ctx.getString(R.string.status_done)
                    setTextColor(ctx.getColor(R.color.statusGreen))
                }
                TransferStatus.FAILED -> {
                    text = ctx.getString(R.string.status_failed)
                    setTextColor(ctx.getColor(R.color.statusRed))
                }
                TransferStatus.CANCELLED -> {
                    text = ctx.getString(R.string.status_cancelled)
                    setTextColor(ctx.getColor(R.color.textSecondary))
                }
                else -> {
                    text = item.status.name
                    setTextColor(ctx.getColor(R.color.statusOrange))
                }
            }
        }

        val canResend = item.direction == TransferDirection.SEND &&
                item.peerIp.isNotEmpty() &&
                item.status in listOf(TransferStatus.COMPLETED, TransferStatus.FAILED, TransferStatus.CANCELLED)
        holder.resend.visibility = if (canResend) View.VISIBLE else View.GONE
        holder.resend.setOnClickListener { onResend?.invoke(item) }

        holder.itemView.contentDescription = ctx.getString(
            R.string.cd_history_item,
            item.filename,
            FileTransferManager.formatSize(item.totalBytes)
        )

        val bgColor = if (position % 2 == 0) android.graphics.Color.TRANSPARENT else ctx.getColor(R.color.highlight)
        holder.itemView.setBackgroundColor(bgColor)
    }

    private class HistoryDiffCallback : DiffUtil.ItemCallback<TransferRecord>() {
        override fun areItemsTheSame(old: TransferRecord, new: TransferRecord) = old.id == new.id
        override fun areContentsTheSame(old: TransferRecord, new: TransferRecord) = old == new
    }
}
