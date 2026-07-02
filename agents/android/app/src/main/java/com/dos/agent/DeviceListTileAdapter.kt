package com.dos.agent

import android.view.LayoutInflater
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.interpolator.view.animation.FastOutSlowInInterpolator
import androidx.recyclerview.widget.DiffUtil
import androidx.recyclerview.widget.ListAdapter
import androidx.recyclerview.widget.RecyclerView

class DeviceListTileAdapter(
    private val onDeviceClick: (PeerItem) -> Unit,
    private val onDeviceLongClick: (PeerItem) -> Unit
) : ListAdapter<PeerItem, DeviceListTileAdapter.ViewHolder>(DeviceDiffCallback()) {

    class ViewHolder(itemView: View) : RecyclerView.ViewHolder(itemView) {
        val avatar: TextView = itemView.findViewById(R.id.tvDeviceAvatar)
        val name: TextView = itemView.findViewById(R.id.tvDeviceName)
        val subtitle: TextView = itemView.findViewById(R.id.tvDeviceSubtitle)
        val badge: TextView = itemView.findViewById(R.id.tvDeviceBadge)
    }

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
        val view = LayoutInflater.from(parent.context)
            .inflate(R.layout.item_device_list_tile, parent, false)
        return ViewHolder(view)
    }

    override fun onBindViewHolder(holder: ViewHolder, position: Int) {
        val peer = getItem(position)
        val ctx = holder.itemView.context

        // Avatar emoji based on platform
        holder.avatar.text = peer.deviceEmoji  // Use PeerItem's emoji property

        // Device name
        holder.name.text = peer.displayName

        // Subtitle: IP + platform
        val platformLabel = peer.platformLabel
        holder.subtitle.text = if (platformLabel.isNotEmpty()) {
            "${peer.host} · $platformLabel"
        } else {
            peer.host
        }

        // Badge: "Trusted" or "Selected"
        when {
            peer.isSelected -> {
                holder.badge.text = "Selected"
                holder.badge.visibility = View.VISIBLE
            }
            peer.isTrusted -> {
                holder.badge.text = "Trusted"
                holder.badge.visibility = View.VISIBLE
            }
            else -> holder.badge.visibility = View.GONE
        }

        // Entrance animation (first time only)
        if (holder.itemView.tag == null) {
            holder.itemView.tag = "animated"
            holder.itemView.alpha = 0f
            holder.itemView.translationY = 20f
            holder.itemView.animate()
                .alpha(1f)
                .translationY(0f)
                .setDuration(300)
                .setStartDelay((position * 50L).coerceAtMost(300L))
                .setInterpolator(FastOutSlowInInterpolator())
                .start()
        }

        holder.itemView.setOnClickListener { onDeviceClick(peer) }
        holder.itemView.setOnLongClickListener {
            onDeviceLongClick(peer)
            true
        }
        holder.itemView.setOnTouchListener { v, event ->
            when (event.action) {
                MotionEvent.ACTION_DOWN -> v.animate().scaleX(0.97f).scaleY(0.97f).setDuration(100).start()
                MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> v.animate().scaleX(1f).scaleY(1f).setDuration(150).start()
            }
            false
        }
    }

    private class DeviceDiffCallback : DiffUtil.ItemCallback<PeerItem>() {
        override fun areItemsTheSame(old: PeerItem, new: PeerItem) = old.host == new.host
        override fun areContentsTheSame(old: PeerItem, new: PeerItem) = old == new
    }
}
