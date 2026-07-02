package com.dos.agent

import android.animation.ArgbEvaluator
import android.animation.ValueAnimator
import android.graphics.PorterDuff
import android.graphics.PorterDuffColorFilter
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.ImageButton
import android.widget.ImageView
import android.widget.ProgressBar
import android.widget.TextView
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView

class TransferListAdapter(
    private val onCancel: (String) -> Unit,
    private val onRetry: ((String) -> Unit)? = null
) : ListAdapter<TransferState, TransferListAdapter.ViewHolder>(TransferDiffCallback()) {

    class ViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
        val fileIcon: ImageView = itemView.findViewById(R.id.transferFileIcon)
        val direction: TextView = itemView.findViewById(R.id.transferDirection)
        val filename: TextView = itemView.findViewById(R.id.transferFilename)
        val progressFill: View = itemView.findViewById(R.id.transferProgressFill)
        val progressText: TextView = itemView.findViewById(R.id.transferProgressText)
        val speedText: TextView = itemView.findViewById(R.id.transferSpeedText)
        val sparkline: ProgressBar = itemView.findViewById(R.id.transferSparkline)
        val cancelBtn: ImageButton = itemView.findViewById(R.id.transferCancelBtn)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.transfer_item, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val item = getItem(position)
        val ctx = holder.itemView.context

        holder.direction.text = if (item.direction == TransferDirection.SEND) "⬆" else "⬇"

        // File type icon
        val ext = item.filename.substringAfterLast('.', "").lowercase()
        val tintColor = when (ext) {
            "jpg", "jpeg", "png", "gif", "webp", "bmp" -> R.color.statusGreen
            "mp4", "mov", "avi", "mkv" -> R.color.statusBlue
            "mp3", "wav", "flac", "aac" -> R.color.statusPurple
            "pdf", "doc", "docx" -> R.color.statusOrange
            "zip", "rar", "7z" -> R.color.statusYellow
            "apk" -> R.color.statusRed
            else -> null
        }
        if (tintColor != null) {
            holder.fileIcon.setImageResource(R.drawable.ic_file_generic)
            holder.fileIcon.colorFilter = PorterDuffColorFilter(
                ctx.getColor(tintColor), PorterDuff.Mode.SRC_IN
            )
            holder.fileIcon.visibility = View.VISIBLE
            holder.direction.visibility = View.GONE
        } else {
            holder.fileIcon.visibility = View.GONE
            holder.direction.visibility = View.VISIBLE
        }

        val status = SealedTransferStatus.from(item)

        when (status) {
            is SealedTransferStatus.Success -> {
                holder.filename.text = "✓ ${item.filename}"
                holder.filename.setTextColor(ctx.getColor(R.color.statusGreen))
                animateSuccessFlash(holder.itemView)
                holder.sparkline.visibility = View.GONE
            }
            is SealedTransferStatus.Failed -> {
                holder.filename.text = "✗ ${item.filename}"
                holder.filename.setTextColor(ctx.getColor(R.color.statusRed))
                MicroInteractions.shake(holder.itemView)
                holder.speedText.text = status.error
                holder.sparkline.visibility = View.GONE
            }
            is SealedTransferStatus.Sending -> {
                holder.filename.text = item.filename
                holder.filename.setTextColor(ctx.getColor(R.color.textPrimary))
                holder.speedText.text = if (status.eta.isNotEmpty()) {
                    "${status.speed} · ${status.eta}"
                } else {
                    status.speed
                }
                holder.sparkline.visibility = View.VISIBLE
                holder.sparkline.progress = (status.progress * 100).toInt()
            }
            else -> {
                holder.filename.text = item.filename
                holder.filename.setTextColor(ctx.getColor(R.color.textPrimary))
                holder.sparkline.visibility = View.GONE
            }
        }

        // Smooth animated progress bar
        val parentWidth = (holder.progressFill.parent as? View)?.measuredWidth ?: 0
        val targetWidth = if (item.totalBytes > 0 && parentWidth > 0) {
            (item.progressPercent / 100f * parentWidth).toInt()
        } else 0

        if (item.totalBytes > 0 && targetWidth > 0) {
            val anim = ValueAnimator.ofInt(0, targetWidth).apply {
                duration = Anim.progressBarMs
                interpolator = Anim.fastOutSlowIn
                addUpdateListener { va ->
                    holder.progressFill.layoutParams.width = (va.animatedValue as Int).coerceAtLeast(0)
                    holder.progressFill.requestLayout()
                }
            }
            anim.start()
        }

        val transferred = FileTransferManager.formatSize(item.transferredBytes)
        val total = FileTransferManager.formatSize(item.totalBytes)
        holder.progressText.text = "$transferred / $total"

        when (status) {
            is SealedTransferStatus.Success -> {
                holder.cancelBtn.visibility = View.GONE
            }
            is SealedTransferStatus.Failed -> {
                holder.cancelBtn.setImageResource(R.drawable.ic_sync)
                holder.cancelBtn.setOnClickListener { onRetry?.invoke(item.id) }
                holder.cancelBtn.contentDescription = "Retry transfer"
                holder.cancelBtn.visibility = View.VISIBLE
            }
            else -> {
                holder.cancelBtn.setOnClickListener { onCancel(item.id) }
                holder.cancelBtn.contentDescription = "Cancel transfer"
                holder.cancelBtn.visibility = View.VISIBLE
            }
        }
    }

    private fun animateSuccessFlash(view: View) {
        val orig = view.background ?: return
        val flash = ValueAnimator.ofObject(ArgbEvaluator(), 0x00FFFFFF, 0x22FFFFFF, 0x00FFFFFF).apply {
            duration = Anim.flashMs
            interpolator = Anim.fastOutSlowIn
            addUpdateListener { view.setBackgroundColor(it.animatedValue as Int) }
            addListener(object : android.animation.AnimatorListenerAdapter() {
                override fun onAnimationEnd(animation: android.animation.Animator) {
                    view.background = orig
                }
            })
        }
        flash.start()
    }

    override fun onAttachedToRecyclerView(recyclerView: RecyclerView) {
        recyclerView.itemAnimator?.apply {
            addDuration = Anim.listChangeMs
            removeDuration = Anim.listChangeMs
            moveDuration = Anim.listChangeMs
            changeDuration = Anim.listChangeMs
        }
    }

    private class TransferDiffCallback : DiffUtil.ItemCallback<TransferState>() {
        override fun areItemsTheSame(old: TransferState, new: TransferState) = old.id == new.id
        override fun areContentsTheSame(old: TransferState, new: TransferState): Boolean {
            return old.status == new.status &&
                    old.transferredBytes == new.transferredBytes &&
                    old.speedSamples.size == new.speedSamples.size
        }
    }
}
