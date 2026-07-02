package com.dos.agent

import android.animation.ArgbEvaluator
import android.animation.ValueAnimator
import android.content.res.ColorStateList
import android.graphics.Color
import android.view.View
import android.widget.TextView

/**
 * Visual identity utilities — the signature look that makes the app recognizable.
 *
 * == Visual Rules ==
 *   1. brandCyan ONLY for active/primary states — never decoration
 *   2. Cards: single corner radius (12dp), two elevation levels (resting 0dp, elevated 4dp)
 *   3. Active elements get a subtle cyan glow tint
 *   4. Color rhythm: idle=gray, active=cyan, success=green, error=red
 */
object VisualIdentity {

    /**
     * Applies a subtle cyan glow tint to any view background.
     * This is the signature "active" state — barely perceptible but unmistakable.
     */
    fun applyActiveGlow(view: View) {
        val origTint = view.backgroundTintList
        val glow = ValueAnimator.ofObject(
            ArgbEvaluator(),
            Color.TRANSPARENT,
            Color.argb(12, 0, 229, 255),  // brandCyan at 5% opacity
            Color.TRANSPARENT
        ).apply {
            duration = 1200
            repeatCount = ValueAnimator.INFINITE
            repeatMode = ValueAnimator.REVERSE
            addUpdateListener { va ->
                view.setBackgroundColor(va.animatedValue as Int)
            }
            start()
        }
        view.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
            override fun onViewAttachedToWindow(v: View) {}
            override fun onViewDetachedFromWindow(v: View) {
                glow.cancel()
                view.backgroundTintList = origTint
            }
        })
    }

    /**
     * Colors a metric value based on threshold range.
     *   good    → green   (< 70%)
     *   warning → amber   (70–90%)
     *   critical→ red     (> 90%)
     */
    fun colorForMetric(percent: Float, textView: TextView) {
        val color = when {
            percent < 0.7f -> textView.context.getColor(R.color.statusGreen)
            percent < 0.9f -> textView.context.getColor(R.color.statusOrange)
            else -> textView.context.getColor(R.color.statusRed)
        }
        textView.setTextColor(color)
    }
}
