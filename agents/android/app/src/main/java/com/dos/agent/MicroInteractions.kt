package com.dos.agent

import android.animation.AnimatorSet
import android.animation.ObjectAnimator
import android.view.HapticFeedbackConstants
import android.view.View
import kotlin.math.abs

object MicroInteractions {

    fun press(view: View, onRelease: (() -> Unit)? = null) {
        view.animate().cancel()
        view.animate()
            .scaleX(Anim.pressScale)
            .scaleY(Anim.pressScale)
            .setDuration(Anim.pressMs)
            .setInterpolator(Anim.fastOutLinearIn)
            .withEndAction {
                view.animate()
                    .scaleX(1f)
                    .scaleY(1f)
                    .setDuration(Anim.releaseMs)
                    .setInterpolator(Anim.overshoot)
                    .withEndAction(onRelease)
                    .start()
            }
            .start()
    }

    fun successBump(view: View) {
        val scaleUpX = ObjectAnimator.ofFloat(view, View.SCALE_X, 1f, Anim.successBumpScale).setDuration(100)
        val scaleUpY = ObjectAnimator.ofFloat(view, View.SCALE_Y, 1f, Anim.successBumpScale).setDuration(100)
        val scaleDownX = ObjectAnimator.ofFloat(view, View.SCALE_X, Anim.successBumpScale, 1f).setDuration(200)
        val scaleDownY = ObjectAnimator.ofFloat(view, View.SCALE_Y, Anim.successBumpScale, 1f).setDuration(200)
        AnimatorSet().apply {
            playSequentially(
                AnimatorSet().apply { playTogether(scaleUpX, scaleUpY) },
                AnimatorSet().apply { playTogether(scaleDownX, scaleDownY) }
            )
            interpolator = Anim.accelerateDecelerate
            start()
        }
    }

    fun shake(view: View) {
        val shake = ObjectAnimator.ofFloat(view, View.TRANSLATION_X, 0f, -12f, 12f, -8f, 8f, -4f, 4f, 0f)
        shake.duration = Anim.shakeMs
        shake.interpolator = Anim.fastOutSlowIn
        shake.start()
    }

    fun fadeIn(view: View, delayMs: Long = 0) {
        view.alpha = 0f
        view.translationY = Anim.slideDp
        view.visibility = View.VISIBLE
        view.animate()
            .alpha(1f)
            .translationY(0f)
            .setDuration(Anim.fadeInMs)
            .setStartDelay(delayMs)
            .setInterpolator(Anim.fastOutSlowIn)
            .start()
    }

    fun fadeOut(view: View, onEnd: (() -> Unit)? = null) {
        view.animate()
            .alpha(0f)
            .translationY(-Anim.slideDp)
            .setDuration(Anim.fadeOutMs)
            .setInterpolator(Anim.fastOutSlowIn)
            .withEndAction {
                view.visibility = View.GONE
                view.translationY = 0f
                onEnd?.invoke()
            }
            .start()
    }

    // --- Haptic mapping ---
    fun hapticTick(view: View) {
        try {
            view.performHapticFeedback(
                HapticFeedbackConstants.KEYBOARD_TAP,
                HapticFeedbackConstants.FLAG_IGNORE_GLOBAL_SETTING
            )
        } catch (_: Exception) {}
    }

    fun hapticSuccess(view: View) {
        try {
            view.performHapticFeedback(
                HapticFeedbackConstants.CONFIRM,
                HapticFeedbackConstants.FLAG_IGNORE_GLOBAL_SETTING
            )
        } catch (_: Exception) {}
    }

    fun hapticError(view: View) {
        try {
            view.performHapticFeedback(
                HapticFeedbackConstants.REJECT,
                HapticFeedbackConstants.FLAG_IGNORE_GLOBAL_SETTING
            )
        } catch (_: Exception) {}
    }
}
