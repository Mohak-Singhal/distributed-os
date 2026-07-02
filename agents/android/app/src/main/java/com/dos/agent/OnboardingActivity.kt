package com.dos.agent

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.recyclerview.widget.RecyclerView
import androidx.viewpager2.widget.ViewPager2

class OnboardingActivity : AppCompatActivity() {

    private lateinit var pager: ViewPager2
    private lateinit var dotsIndicator: LinearLayout
    private lateinit var btnNext: Button
    private lateinit var btnSkip: TextView
    private lateinit var pages: List<Triple<String, String, String>>

    private val dots = mutableListOf<View>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_onboarding)

        pages = listOf(
            Triple("\uD83D\uDCE4", getString(R.string.onboarding_title_1), getString(R.string.onboarding_desc_1)),
            Triple("\uD83D\uDCF1", getString(R.string.onboarding_title_2), getString(R.string.onboarding_desc_2)),
            Triple("\uD83D\uDD12", getString(R.string.onboarding_title_3), getString(R.string.onboarding_desc_3))
        )

        pager = findViewById(R.id.onboardingPager)
        dotsIndicator = findViewById(R.id.dotsIndicator)
        btnNext = findViewById(R.id.btnNext)
        btnSkip = findViewById(R.id.btnSkip)

        pager.adapter = OnboardingViewAdapter(layoutInflater, pages)

        setupDots()
        pager.registerOnPageChangeCallback(object : ViewPager2.OnPageChangeCallback() {
            override fun onPageSelected(position: Int) {
                updateDots(position)
                btnNext.text = if (position == pages.lastIndex)
                    getString(R.string.onboarding_get_started)
                else
                    getString(R.string.onboarding_next)
            }
        })

        btnNext.setOnClickListener {
            val current = pager.currentItem
            if (current < pages.lastIndex) {
                pager.currentItem = current + 1
            } else {
                completeOnboarding()
            }
        }

        btnSkip.setOnClickListener { completeOnboarding() }
    }

    private fun completeOnboarding() {
        getSharedPreferences("pdos_prefs", Context.MODE_PRIVATE)
            .edit()
            .putBoolean("onboarding_done", true)
            .apply()
        startActivity(Intent(this, MainActivity::class.java))
        finish()
    }

    private fun setupDots() {
        dotsIndicator.removeAllViews()
        dots.clear()
        for (i in pages.indices) {
            val dot = View(this)
            val px = { dp: Int -> (dp * resources.displayMetrics.density).toInt() }
            val lp = LinearLayout.LayoutParams(px(10), px(10))
            lp.setMargins(px(6), 0, px(6), 0)
            dot.layoutParams = lp
            dot.setBackgroundResource(android.R.drawable.presence_offline)
            dot.alpha = 0.4f
            dotsIndicator.addView(dot)
            dots.add(dot)
        }
        updateDots(0)
    }

    private fun updateDots(selected: Int) {
        dots.forEachIndexed { i, dot ->
            dot.alpha = if (i == selected) 1.0f else 0.4f
        }
    }
}

class OnboardingViewAdapter(
    private val inflater: LayoutInflater,
    private val pages: List<Triple<String, String, String>>
) : RecyclerView.Adapter<OnboardingViewAdapter.PageHolder>() {

    override fun getItemCount() = pages.size

    override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): PageHolder {
        val view = inflater.inflate(R.layout.page_onboarding, parent, false)
        return PageHolder(view)
    }

    override fun onBindViewHolder(holder: PageHolder, position: Int) {
        val (icon, title, desc) = pages[position]
        holder.icon.text = icon
        holder.title.text = title
        holder.description.text = desc
    }

    class PageHolder(view: View) : RecyclerView.ViewHolder(view) {
        val icon: TextView = view.findViewById(R.id.onboardingIcon)
        val title: TextView = view.findViewById(R.id.onboardingTitle)
        val description: TextView = view.findViewById(R.id.onboardingDescription)
    }
}
