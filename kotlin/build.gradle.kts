plugins {
    kotlin("jvm") version "2.1.10"
    application
}

group = "ai.muonium"
version = file("../VERSION").readText().trim()

repositories {
    mavenCentral()
}

kotlin {
    jvmToolchain(21)
}

val generatedSrcDir = layout.buildDirectory.dir("generated/kotlin")

val generateVersion by tasks.registering {
    val versionFile = file("../VERSION")
    val outputDir = generatedSrcDir
    inputs.file(versionFile)
    outputs.dir(outputDir)
    doLast {
        val ver = versionFile.readText().trim()
        val parts = ver.split(".")
        val dir = outputDir.get().asFile.resolve("ai/muonium/muongit")
        dir.mkdirs()
        dir.resolve("GeneratedVersion.kt").writeText(
            """package ai.muonium.muongit
            |
            |/** Auto-generated from VERSION file — do not edit */
            |internal object GeneratedVersion {
            |    const val STRING = "$ver"
            |    const val MAJOR = ${parts[0]}
            |    const val MINOR = ${parts[1]}
            |    const val PATCH = ${parts[2]}
            |}
            |""".trimMargin()
        )
    }
}

sourceSets {
    main {
        kotlin.srcDir("src/main/kotlin")
        kotlin.srcDir(generatedSrcDir)
    }
    test {
        kotlin.srcDir("src/test/kotlin")
    }
    create("bench") {
        kotlin.srcDir("src/bench/kotlin")
        compileClasspath += sourceSets.main.get().output + sourceSets.main.get().compileClasspath
        runtimeClasspath += sourceSets.main.get().output + sourceSets.main.get().runtimeClasspath
    }
    create("conformance") {
        kotlin.srcDir("src/conformance/kotlin")
        compileClasspath += sourceSets.main.get().output + sourceSets.main.get().compileClasspath
        runtimeClasspath += sourceSets.main.get().output + sourceSets.main.get().runtimeClasspath
    }
}

tasks.named("compileKotlin") { dependsOn(generateVersion) }
tasks.matching { it.name == "compileConformanceKotlin" }.configureEach { dependsOn(generateVersion) }

dependencies {
    testImplementation(kotlin("test"))
}

tasks.test {
    useJUnitPlatform()
}

tasks.register<JavaExec>("bench") {
    description = "Run MuonGit benchmarks"
    mainClass.set("ai.muonium.muongit.BenchmarkKt")
    classpath = sourceSets["bench"].runtimeClasspath
}

tasks.register<JavaExec>("runConformance") {
    description = "Run the MuonGit cross-implementation conformance helper"
    mainClass.set("ai.muonium.muongit.ConformanceKt")
    classpath = sourceSets["conformance"].runtimeClasspath
    doFirst {
        args = (System.getenv("MUONGIT_CONFORMANCE_ARGS") ?: "")
            .lineSequence()
            .filter { it.isNotBlank() }
            .toList()
    }
}
