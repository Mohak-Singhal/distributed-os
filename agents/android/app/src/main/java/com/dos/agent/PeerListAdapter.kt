package com.dos.agent

import android.view.LayoutInflater
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup

import android.widget.ImageView
import android.widget.TextView
import androidx.interpolator.view.animation.FastOutSlowInInterpolator
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView

class PeerListAdapter(
    private val onPeerClick: (PeerItem) -> Unit,
    private val onPeerLongClick: (PeerItem) -> Unit
) : ListAdapter<PeerItem, PeerListAdapter.ViewHolder>(PeerDiffCallback()) {

    var isScanning: Boolean = false
        private set

    fun setScanning(scanning: Boolean) {
        isScanning = scanning
        notifyDataSetChanged()
    }

    class ViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
        val avatarImage: ImageView? = itemView.findViewById(R.id.ivDeviceIcon)
        val avatarIcon: TextView = itemView.findViewById(R.id.tvAvatarIcon)
        val deviceName: TextView = itemView.findViewById(R.id.tvDeviceName)
        val devicePlatform: TextView = itemView.findViewById(R.id.tvDevicePlatform)
        val statusText: TextView = itemView.findViewById(R.id.tvStatus)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_peer_device, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val peer = getItem(position)
        val ctx = holder.itemView.context

        val iconRes = platformDrawableRes(peer.platform)
        val avatarTarget = holder.avatarImage
        if (avatarTarget != null) {
            avatarTarget.setImageResource(iconRes)
            avatarTarget.visibility = View.VISIBLE
            holder.avatarIcon.visibility = View.GONE
        } else {
            holder.avatarIcon.setCompoundDrawablesRelativeWithIntrinsicBounds(0, iconRes, 0, 0)
            holder.avatarIcon.text = ""
            holder.avatarIcon.visibility = View.VISIBLE
        }

        holder.deviceName.text = peer.displayName
        holder.devicePlatform.text = peer.platformLabel

        if (peer.isSelected || peer.isTrusted) {
            holder.statusText.text = if (peer.isSelected) "Selected" else "Trusted"
            holder.statusText.setTextColor(ctx.getColor(R.color.statusGreen))
            holder.statusText.visibility = View.VISIBLE
        } else {
            holder.statusText.visibility = View.GONE
        }

        // Entrance animation
        if (holder.itemView.tag == null) {
            holder.itemView.tag = "animated"
            holder.itemView.alpha = 0f
            holder.itemView.translationY = 20f
            holder.itemView.animate()
                .alpha(1f)
                .translationY(0f)
                .setDuration(Anim.fadeInMs)
                .setStartDelay((position * 50L).coerceAtMost(300L))
                .setInterpolator(FastOutSlowInInterpolator())
                .start()
        }

        // Scanning pulse on first item
        if (isScanning && position == 0) {
            val pulseView = holder.avatarImage ?: holder.avatarIcon
            pulseView.animate().cancel()
            pulseView.alpha = 1f
            pulseView.animate()
                .alpha(0.4f)
                .setDuration(Anim.radarPulseMs / 2)
                .setInterpolator(FastOutSlowInInterpolator())
                .withEndAction {
                    pulseView.animate()
                        .alpha(1f)
                        .setDuration(Anim.radarPulseMs / 2)
                        .setInterpolator(FastOutSlowInInterpolator())
                        .start()
                }
                .start()
        }

        holder.itemView.setOnClickListener { onPeerClick(peer) }
        holder.itemView.setOnLongClickListener {
            onPeerLongClick(peer)
            true
        }
        holder.itemView.setOnTouchListener { v, event ->
            when (event.action) {
                MotionEvent.ACTION_DOWN -> v.animate().scaleX(0.95f).scaleY(0.95f).setDuration(Anim.pressMs).start()
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> v.animate().scaleX(1f).scaleY(1f).setDuration(Anim.releaseMs).start()
            }
            false
        }
    }

    private fun platformDrawableRes(platform: String): Int {
        return when (platform.lowercase()) {
            "android", "phone" -> R.drawable.ic_device_phone
            "mac", "macos", "laptop" -> R.drawable.ic_device_laptop
            "desktop", "windows", "linux" -> R.drawable.ic_device_desktop
            else -> R.drawable.ic_device_phone
        }
    }

    private class PeerDiffCallback : DiffUtil.ItemCallback<PeerItem>() {
        override fun areItemsTheSame(old: PeerItem, new: PeerItem) = old.host == new.host
        override fun areContentsTheSame(old: PeerItem, new: PeerItem) = old == new
    }
}
