package com.dos.agent

import android.animation.ObjectAnimator
import android.content.Context
import android.util.AttributeSet
import android.view.LayoutInflater
import android.view.MotionEvent
import android.view.View
import android.view.VelocityTracker
import android.widget.Button
import android.widget.FrameLayout
import android.widget.TextView
import kotlin.math.abs
import java.util.LinkedList

class StatusBanner @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : FrameLayout(context, attrs, defStyleAttr) {

    private val message: TextView
    private val retryBtn: Button
    private val queue = LinkedList<BannerEntry>()
    private var isShowing = false
    private var dismissRunnable: Runnable? = null

    // Gesture tracking
    private var touchStartX = 0f
    private var touchStartY = 0f
    private var originalX = 0f
    private var velocityTracker: VelocityTracker? = null
    private var isSwiping = false

    private data class BannerEntry(
        val status: SealedTransferStatus,
        val retry: (() -> Unit)?
    )

    init {
        val root = LayoutInflater.from(context).inflate(R.layout.status_banner, this, true)
        message = root.findViewById(R.id.bannerMessage)
        retryBtn = root.findViewById(R.id.bannerRetry)
        retryBtn.setOnClickListener { currentRetry?.invoke() }

        // Start hidden
        alpha = 0f
        visibility = GONE
        translationY = -Anim.slideDp

        setOnTouchListener { _, event ->
            if (visibility != VISIBLE) return@setOnTouchListener false
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    touchStartX = event.x
                    touchStartY = event.y
                    originalX = translationX
                    velocityTracker = VelocityTracker.obtain()
                    velocityTracker?.addMovement(event)
                    isSwiping = false
                    true
                }
                MotionEvent.ACTION_MOVE -> {
                    val dx = event.x - touchStartX
                    val dy = abs(event.y - touchStartY)
                    velocityTracker?.addMovement(event)
                    velocityTracker?.computeCurrentVelocity(1000)

                    if (!isSwiping && abs(dx) > dy && abs(dx) > 8) {
                        isSwiping = true
                    }
                    if (isSwiping && dx > 0) {
                        translationX = originalX + dx * 0.4f
                        alpha = 1f - (dx / width).coerceIn(0f, 0.5f)
                    }
                    true
                }
                MotionEvent.ACTION_UP -> {
                    val dx = event.x - touchStartX
                    val velocityX = velocityTracker?.xVelocity ?: 0f
                    velocityTracker?.recycle()
                    velocityTracker = null

                    if (isSwiping && (dx > Anim.swipeDismissThresholdDp || velocityX > 800f)) {
                        dismiss()
                    } else if (isSwiping) {
                        animate()
                            .translationX(0f)
                            .alpha(1f)
                            .setDuration(Anim.releaseMs)
                            .setInterpolator(Anim.overshoot)
                            .start()
                    }
                    isSwiping = false
                    true
                }
                MotionEvent.ACTION_CANCEL -> {
                    velocityTracker?.recycle()
                    velocityTracker = null
                    if (isSwiping) {
                        animate().translationX(0f).alpha(1f).setDuration(Anim.releaseMs).start()
                    }
                    isSwiping = false
                    true
                }
                else -> false
            }
        }
        isClickable = true
    }

    private var currentRetry: (() -> Unit)? = null

    fun show(status: SealedTransferStatus, retry: (() -> Unit)? = null) {
        if (isShowing) {
            queue.add(BannerEntry(status, retry))
            return
        }
        showNow(status, retry)
    }

    private fun showNow(status: SealedTransferStatus, retry: (() -> Unit)? = null) {
        currentRetry = retry
        dismissRunnable?.let { removeCallbacks(it) }
        dismissRunnable = null
        isShowing = true

        when (status) {
            is SealedTransferStatus.Idle -> {
                dismiss()
                return
            }
            is SealedTransferStatus.Sending -> {
                message.text = status.message
                message.setTextColor(context.getColor(R.color.statusOrange))
                retryBtn.visibility = GONE
            }
            is SealedTransferStatus.Success -> {
                message.text = status.message
                message.setTextColor(context.getColor(R.color.statusGreen))
                retryBtn.visibility = GONE
                dismissRunnable = Runnable { dismiss() }
                postDelayed(dismissRunnable, Anim.autoDismissMs)
                MicroInteractions.successBump(this)
                MicroInteractions.hapticSuccess(this)
            }
            is SealedTransferStatus.Failed -> {
                message.text = status.error
                message.setTextColor(context.getColor(R.color.statusRed))
                retryBtn.visibility = if (retry != null) VISIBLE else GONE
                MicroInteractions.shake(this)
                MicroInteractions.hapticError(this)
            }
        }

        if (visibility != VISIBLE) {
            alpha = 0f
            translationY = -Anim.slideDp
            visibility = VISIBLE
            animate()
                .alpha(1f)
                .translationY(0f)
                .setDuration(Anim.bannerMs)
                .setInterpolator(Anim.overshoot)
                .start()
        }
    }

    /**
     * Shows a peek of the next queued item behind the current banner.
     */
    private fun previewNext() {
        val next = queue.peek() ?: return
        // Store the peek in a shadow state (optional visual enhancement)
    }

    private fun showNext() {
        val entry = queue.poll()
        if (entry == null) {
            isShowing = false
            return
        }
        showNow(entry.status, entry.retry)
    }

    private fun dismiss() {
        animate()
            .alpha(0f)
            .translationY(-Anim.slideDp)
            .setDuration(Anim.fadeOutMs)
            .setInterpolator(Anim.fastOutSlowIn)
            .withEndAction {
                visibility = GONE
                translationX = 0f
                translationY = 0f
                alpha = 1f
                isShowing = false
                previewNext()
                showNext()
            }
            .start()
    }

    fun clear() {
        queue.clear()
        dismissRunnable?.let { removeCallbacks(it) }
        dismissRunnable = null
        dismiss()
    }
}
