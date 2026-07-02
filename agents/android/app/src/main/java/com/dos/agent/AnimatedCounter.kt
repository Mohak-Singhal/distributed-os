package com.dos.agent

import android.animation.ValueAnimator
import android.widget.TextView

/**
 * Animates a numeric TextView from one value to another using smooth easing.
 */
object AnimatedCounter {

    private val activeAnimations = mutableMapOf<TextView, ValueAnimator>()

    fun countTo(view: TextView, target: Long, suffix: String = "", prefix: String = "") {
        activeAnimations[view]?.cancel()
        val current = view.text.toString()
            .replace(prefix, "")
            .replace(suffix, "")
            .replace(",", "")
            .toLongOrNull() ?: 0L

        val anim = ValueAnimator.ofFloat(current.toFloat(), target.toFloat()).apply {
            duration = Anim.progressBarMs
            interpolator = Anim.fastOutSlowIn
            addUpdateListener { va ->
                val value = (va.animatedValue as Float).toLong()
                view.text = "$prefix${formatNumber(value)}$suffix"
            }
            addListener(object : android.animation.AnimatorListenerAdapter() {
                override fun onAnimationEnd(animation: android.animation.Animator) {
                    view.text = "$prefix${formatNumber(target)}$suffix"
                    activeAnimations.remove(view)
                }
            })
            start()
        }
        activeAnimations[view] = anim
    }

    private fun formatNumber(value: Long): String {
        return when {
            value >= 1_000_000_000 -> String.format("%.1fB", value / 1_000_000_000f)
            value >= 1_000_000 -> String.format("%.1fM", value / 1_000_000f)
            value >= 1_000 -> String.format("%.1fK", value / 1_000f)
            else -> value.toString()
        }
    }
}
